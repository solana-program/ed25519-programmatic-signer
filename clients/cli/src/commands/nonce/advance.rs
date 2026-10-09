use {
    crate::{cli::keypair_source_parser, client::Client, output::OutputFormat},
    anyhow::{Context, Result, bail},
    clap::Args,
    serde::{Deserialize, Serialize},
    solana_address::Address,
    solana_clap_v3_utils::input_parsers::signer::SignerSource,
    solana_hash::Hash,
    solana_instruction::Instruction,
    solana_keypair::Keypair,
    solana_message::{MessageHeader, VersionedMessage, v1},
    solana_signer::Signer,
    solana_transaction::Transaction,
    spl_ed25519_signer_client::ProgrammaticSigner,
    std::fmt,
};

#[derive(Debug, Args)]
pub(crate) struct AdvanceCommand {
    /// Address of the SPL Nonce account to advance.
    pub(crate) nonce_account: Address,

    /// Signer source for the nonce authority, or for the cold authority whose derived
    /// ProgrammaticSigner PDA is the nonce authority. Accepts a keypair file, usb:// URL,
    /// prompt:// URL, or ASK. Defaults to the configured keypair.
    #[clap(long, value_parser = keypair_source_parser())]
    pub(crate) nonce_authority: Option<SignerSource>,
}

pub(super) async fn run(
    command: AdvanceCommand,
    client: &Client,
    output: OutputFormat,
) -> Result<String> {
    let authority = client
        .load_signer_or_config_default(command.nonce_authority.as_ref(), "nonce authority")?;
    let authority_address = authority
        .try_pubkey()
        .context("failed to read nonce authority pubkey")?;
    let fee_payer = client.fee_payer()?;
    let fee_payer_address = fee_payer
        .try_pubkey()
        .context("failed to read fee payer pubkey")?;

    let nonce_account = command.nonce_account;
    let previous = client.nonce_account(&nonce_account).await?.state;
    let pda =
        ProgrammaticSigner::derive_address(&spl_ed25519_signer_client::id(), &authority_address);

    let mut signers: Vec<&dyn Signer> = vec![fee_payer.as_ref()];
    let instruction = if previous.authority == authority_address {
        // The authority signs the advance instruction to update the nonce account
        if authority_address != fee_payer_address {
            signers.push(authority.as_ref());
        }
        direct_advance(&authority_address, &nonce_account, previous.nonce)
    } else if previous.authority == pda {
        // The authority signs an authorization message, allowing the PDA to advance the nonce account
        eprintln!(
            "Cold authority {authority_address} signs an empty execution message to advance nonce \
             account {nonce_account}."
        );
        authorized_advance(
            authority.as_ref(),
            &authority_address,
            &pda,
            &nonce_account,
            previous.nonce,
        )?
    } else {
        bail!(
            "nonce account {nonce_account} has authority {}, but the authority signer is \
             {authority_address} and its derived signer is {pda}",
            previous.authority
        );
    };

    let mut transaction = Transaction::new_with_payer(&[instruction], Some(&fee_payer_address));
    let blockhash = client.latest_blockhash().await?;
    transaction
        .try_sign(&signers, blockhash)
        .context("failed to sign nonce advance transaction")?;
    let signature = client.send_and_confirm_transaction(&transaction).await?;

    let account = client
        .nonce_account(&nonce_account)
        .await
        .with_context(|| {
            format!(
                "transaction {signature} confirmed, but failed to read advanced nonce account \
                 {nonce_account}"
            )
        })?;

    output.render(&NonceAdvanceOutput {
        signature,
        nonce_account: nonce_account.to_string(),
        previous_nonce: previous.nonce.to_string(),
        nonce: account.state.nonce.to_string(),
    })
}

/// Build an Advance instruction signed directly by the nonce authority.
fn direct_advance(
    authority: &Address,
    nonce_account: &Address,
    current_nonce: Hash,
) -> Instruction {
    // A random commitment makes the next nonce unpredictable
    let transition_commitment = Hash::new_from_array(Keypair::new().pubkey().to_bytes());
    spl_nonce_client::instruction::advance(
        authority,
        nonce_account,
        current_nonce,
        transition_commitment,
    )
}

/// Build a Submit instruction that advances a nonce owned by a cold authority's
/// ProgrammaticSigner PDA by executing an empty execution message.
fn authorized_advance(
    cold_authority: &dyn Signer,
    cold_authority_address: &Address,
    pda: &Address,
    nonce_account: &Address,
    current_nonce: Hash,
) -> Result<Instruction> {
    // v1 messages need a writable signer. The PDA fills that slot, so the cold authority is the
    // only authorization message signer, and the executor never uses it because the message has
    // no instructions. The advance uses the hash of this message, so the random address
    // makes the next nonce unpredictable.
    let execution_message = v1::Message {
        header: MessageHeader {
            num_required_signatures: 1,
            num_readonly_signed_accounts: 0,
            num_readonly_unsigned_accounts: 1,
        },
        config: v1::TransactionConfig::default(),
        lifetime_specifier: current_nonce,
        account_keys: vec![*pda, Keypair::new().pubkey()],
        instructions: vec![],
    };
    let execute =
        spl_message_executor_client::instruction::execute(nonce_account, pda, &execution_message);
    let VersionedMessage::V1(authorization_message) =
        spl_ed25519_signer_client::message::authorization_message(
            &execute,
            &[*cold_authority_address],
        )
    else {
        unreachable!("authorization_message builds a v1 message");
    };
    authorization_message
        .validate()
        .context("invalid authorization message")?;
    let signature = cold_authority
        .try_sign_message(&authorization_message.serialize())
        .with_context(|| {
            format!("failed to sign authorization message with {cold_authority_address}")
        })?;
    Ok(spl_ed25519_signer_client::instruction::submit(
        vec![signature],
        VersionedMessage::V1(authorization_message),
    ))
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NonceAdvanceOutput {
    pub signature: String,
    pub nonce_account: String,
    pub previous_nonce: String,
    pub nonce: String,
}

impl fmt::Display for NonceAdvanceOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Signature: {}", self.signature)?;
        writeln!(formatter, "Nonce account: {}", self.nonce_account)?;
        writeln!(formatter, "Previous nonce: {}", self.previous_nonce)?;
        write!(formatter, "Nonce: {}", self.nonce)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::{authorized_advance, direct_advance},
        solana_address::Address,
        solana_hash::Hash,
        solana_instruction::{AccountMeta, Instruction},
        solana_keypair::Keypair,
        solana_message::{VersionedMessage, v1},
        solana_signer::Signer,
        spl_ed25519_signer_client::ProgrammaticSigner,
        spl_ed25519_signer_interface::instruction::Instruction as SignerInstruction,
        spl_message_executor_interface::instruction::{
            Instruction as ExecutorInstruction, derive_transition_commitment,
        },
        spl_nonce_interface::instruction::Instruction as NonceInstruction,
    };

    /// Decode a direct Advance and return its transition commitment.
    fn direct_commitment(
        instruction: &Instruction,
        authority: &Address,
        nonce_account: &Address,
        current_nonce: Hash,
    ) -> Hash {
        assert_eq!(instruction.program_id, spl_nonce_interface::id());
        assert_eq!(
            instruction.accounts,
            [
                AccountMeta::new_readonly(*authority, true),
                AccountMeta::new(*nonce_account, false),
            ]
        );
        let NonceInstruction::Advance {
            current_nonce: advanced_nonce,
            transition_commitment,
        } = wincode::deserialize_exact(&instruction.data).unwrap()
        else {
            panic!("expected an Advance instruction");
        };
        assert_eq!(advanced_nonce, current_nonce);
        transition_commitment
    }

    /// Decode an authorized Submit, check the authorization message and its empty execution
    /// message, and return the transition commitment the executor will advance with.
    fn authorized_commitment(
        instruction: &Instruction,
        cold_authority: &Address,
        pda: &Address,
        nonce_account: &Address,
        current_nonce: Hash,
    ) -> Hash {
        assert_eq!(instruction.program_id, spl_ed25519_signer_client::id());
        let SignerInstruction::Submit {
            signatures,
            message: VersionedMessage::V1(authorization_message),
        } = SignerInstruction::try_from_bytes(&instruction.data).unwrap()
        else {
            panic!("expected a v1 Submit instruction");
        };

        // The cold authority is the only authorization message signer.
        assert_eq!(authorization_message.header.num_required_signatures, 1);
        assert_eq!(authorization_message.account_keys[0], *cold_authority);
        let [signature] = signatures.as_slice() else {
            panic!("expected one authorization signature");
        };
        assert!(signature.verify(cold_authority.as_ref(), &authorization_message.serialize()));

        let [execute] = authorization_message.instructions.as_slice() else {
            panic!("expected one Execute instruction");
        };
        let account_keys = &authorization_message.account_keys;
        assert_eq!(
            account_keys[usize::from(execute.program_id_index)],
            spl_message_executor_interface::id()
        );
        let execute_accounts = execute
            .accounts
            .iter()
            .map(|index| account_keys[usize::from(*index)])
            .collect::<Vec<_>>();
        assert_eq!(
            execute_accounts[..3],
            [*pda, *nonce_account, spl_nonce_interface::id()]
        );

        let ExecutorInstruction::Execute(execution_message) =
            ExecutorInstruction::try_from_bytes(&execute.data).unwrap();
        let VersionedMessage::V1(message) = &execution_message else {
            panic!("expected a v1 execution message");
        };
        message.validate().unwrap();
        assert_eq!(message.config, v1::TransactionConfig::default());
        assert_eq!(message.lifetime_specifier, current_nonce);
        assert!(message.instructions.is_empty());
        assert_eq!(message.account_keys[0], *pda);
        derive_transition_commitment(&execution_message)
    }

    #[test]
    fn direct_advance_uses_a_random_commitment() {
        let authority = Address::new_unique();
        let nonce_account = Address::new_unique();
        let current_nonce = Hash::new_unique();

        let commitment = || {
            let instruction = direct_advance(&authority, &nonce_account, current_nonce);
            direct_commitment(&instruction, &authority, &nonce_account, current_nonce)
        };
        let first = commitment();
        let second = commitment();
        assert_ne!(first, second);
        assert_ne!(first, Hash::default());
    }

    #[test]
    fn authorized_advance_uses_a_random_commitment() {
        let cold_authority = Keypair::new();
        let cold_authority_address = cold_authority.pubkey();
        let pda = ProgrammaticSigner::derive_address(
            &spl_ed25519_signer_client::id(),
            &cold_authority_address,
        );
        let nonce_account = Address::new_unique();
        let current_nonce = Hash::new_unique();

        let commitment = || {
            let instruction = authorized_advance(
                &cold_authority,
                &cold_authority_address,
                &pda,
                &nonce_account,
                current_nonce,
            )
            .unwrap();
            authorized_commitment(
                &instruction,
                &cold_authority_address,
                &pda,
                &nonce_account,
                current_nonce,
            )
        };
        assert_ne!(commitment(), commitment());
    }
}
