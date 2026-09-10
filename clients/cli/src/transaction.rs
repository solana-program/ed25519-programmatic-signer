//! Validated transaction files for offline signing and relay commands.
use {
    anyhow::{Context, Result, ensure},
    solana_address::Address,
    solana_hash::Hash,
    solana_message::{VersionedMessage, legacy::Message},
    solana_sanitize::Sanitize,
    solana_signature::Signature,
    solana_signer::Signer,
    solana_transaction::{Transaction, versioned::VersionedTransaction},
    spl_ed25519_signer_client::{
        ProgrammaticSigner, instruction::submit, message::wrapped_message,
    },
    spl_legacy_message_executor_client::instruction::execute,
    spl_legacy_message_executor_interface::instruction::{
        Instruction as ExecutorInstruction, derive_transition_commitment,
    },
    spl_nonce_interface::state::Nonce,
};

// Construction and decoding validate the message and every populated signature once. The
// message stays private and immutable; signing and merging can only fill signature slots.
pub(crate) struct WrappedTransaction {
    transaction: VersionedTransaction,
    inner: Message,
    nonce_account: Address,
}

impl WrappedTransaction {
    pub(crate) fn new(
        inner: Message,
        nonce_account: Address,
        authorities: &[Address],
        submit_signers: &[Address],
        genesis_hash: Hash,
    ) -> Result<Self> {
        validate_message(&inner)?;
        ensure!(
            !authorities.is_empty(),
            "at least one cold authority is required"
        );
        let mut signers = authorities.to_vec();
        signers.extend_from_slice(submit_signers);
        ensure_unique(&signers)?;
        for signer in submit_signers {
            ensure!(
                inner.signer_keys().contains(&signer),
                "submit signer {signer} is not required by inner message"
            );
            ensure!(
                !authorities
                    .iter()
                    .any(|authority| programmatic_signer(authority) == *signer),
                "submit signer {signer} is a programmatic signer PDA"
            );
        }
        for signer in inner.signer_keys() {
            ensure!(
                submit_signers.contains(signer)
                    || authorities
                        .iter()
                        .any(|authority| programmatic_signer(authority) == *signer),
                "required inner signer {signer} is not covered"
            );
        }
        let instruction = execute(&nonce_account, &inner);
        // wrapped_message uses u8 header fields and compiled indices. Check its bounds before
        // calling the existing low-level builder, which expects valid inputs.
        let unsigned = instruction
            .accounts
            .iter()
            .filter(|meta| !signers.contains(&meta.pubkey) && meta.pubkey != instruction.program_id)
            .collect::<Vec<_>>();
        // The high bit of the first legacy header byte is reserved for message versions.
        ensure!(signers.len() < 128, "too many wrapper signers");
        ensure!(
            signers
                .len()
                .saturating_add(unsigned.len())
                .saturating_add(1)
                <= 256,
            "too many wrapper account keys"
        );
        ensure!(
            unsigned.iter().filter(|meta| !meta.is_writable).count() < 255,
            "too many readonly wrapper accounts"
        );
        let mut message = wrapped_message(&instruction, &signers);
        message.set_recent_blockhash(genesis_hash);
        Self::from_transaction(VersionedTransaction {
            signatures: vec![Signature::default(); signers.len()],
            message,
        })
    }

    pub(crate) fn from_json(json: &str) -> Result<Self> {
        let transaction: Transaction =
            serde_json::from_str(json).context("invalid transaction JSON")?;
        Self::from_transaction(transaction.into())
    }

    pub(crate) fn to_json(&self) -> Result<String> {
        let transaction = self
            .transaction
            .clone()
            .into_legacy_transaction()
            .context("only legacy wrapped messages are supported")?;
        serde_json::to_string_pretty(&transaction).context("failed to encode transaction JSON")
    }

    fn from_transaction(transaction: VersionedTransaction) -> Result<Self> {
        transaction
            .sanitize()
            .context("invalid wrapped transaction")?;
        ensure!(
            !transaction.signatures.is_empty(),
            "wrapped transaction has no signers"
        );
        let VersionedMessage::Legacy(message) = &transaction.message else {
            anyhow::bail!("only legacy wrapped messages are supported");
        };
        // JSON decoding does not check the legacy wire format's reserved version bit.
        ensure!(
            message.header.num_required_signatures < 128,
            "too many wrapper signers"
        );
        validate_message(message)?;
        let bytes = message.serialize();
        for (key, signature) in message.account_keys.iter().zip(&transaction.signatures) {
            ensure!(
                *signature == Signature::default() || signature.verify(key.as_ref(), &bytes),
                "invalid signature for {key}"
            );
        }
        let [instruction] = message.instructions.as_slice() else {
            anyhow::bail!("expected one executor instruction");
        };
        ensure!(
            message.account_keys[usize::from(instruction.program_id_index)]
                == spl_legacy_message_executor_interface::id(),
            "invalid executor program id"
        );
        let ExecutorInstruction::Execute(inner) =
            ExecutorInstruction::try_from_bytes(&instruction.data)
                .context("invalid executor instruction data")?;
        validate_message(&inner)?;
        let nonce_index = *instruction
            .accounts
            .first()
            .context("missing nonce account")?;
        let nonce_account = message.account_keys[usize::from(nonce_index)];
        let expected = execute(&nonce_account, &inner);
        ensure!(
            instruction.accounts.len() == expected.accounts.len(),
            "invalid executor accounts"
        );
        for (index, expected) in instruction.accounts.iter().zip(&expected.accounts) {
            let index = usize::from(*index);
            ensure!(
                message.account_keys[index] == expected.pubkey,
                "invalid executor account order"
            );
            ensure!(
                !expected.is_writable || is_writable(message, index),
                "missing writable privilege for {}",
                expected.pubkey
            );
            ensure!(
                !expected.is_signer
                    || message.is_signer(index)
                    || message
                        .signer_keys()
                        .iter()
                        .any(|key| programmatic_signer(key) == expected.pubkey),
                "missing signer privilege for {}",
                expected.pubkey
            );
        }
        Ok(Self {
            transaction,
            inner,
            nonce_account,
        })
    }

    pub(crate) fn inner(&self) -> &Message {
        &self.inner
    }

    pub(crate) fn nonce_account(&self) -> &Address {
        &self.nonce_account
    }

    pub(crate) fn genesis_hash(&self) -> &Hash {
        self.transaction.message.recent_blockhash()
    }

    pub(crate) fn signer_status(&self) -> impl Iterator<Item = (&Address, bool)> {
        self.transaction
            .message
            .static_account_keys()
            .iter()
            .zip(&self.transaction.signatures)
            .map(|(key, signature)| (key, *signature != Signature::default()))
    }

    pub(crate) fn is_fully_signed(&self) -> bool {
        self.signer_status().all(|(_, signed)| signed)
    }

    pub(crate) fn verify_genesis_hash(&self, genesis_hash: &Hash) -> Result<()> {
        ensure!(self.genesis_hash() == genesis_hash, "genesis hash mismatch");
        Ok(())
    }

    pub(crate) fn verify(&self, state: &Nonce, genesis_hash: &Hash) -> Result<()> {
        self.verify_genesis_hash(genesis_hash)?;
        ensure!(self.inner.recent_blockhash == state.nonce, "nonce mismatch");
        ensure!(
            self.inner.signer_keys().contains(&&state.authority),
            "missing nonce authority signer"
        );
        Ok(())
    }

    pub(crate) fn next_nonce(&self) -> Hash {
        Nonce {
            nonce: self.inner.recent_blockhash,
            authority: Address::default(),
        }
        .derive_next_nonce(
            &spl_nonce_interface::id(),
            &self.nonce_account,
            &derive_transition_commitment(&self.inner),
        )
    }

    pub(crate) fn sign(&mut self, signer: &dyn Signer) -> Result<()> {
        let key = signer.try_pubkey()?;
        let index = self
            .signer_status()
            .position(|(address, _)| *address == key)
            .with_context(|| format!("signer {key} is not required"))?;
        let bytes = self.transaction.message.serialize();
        let signature = signer.try_sign_message(&bytes)?;
        ensure!(
            signature.verify(key.as_ref(), &bytes),
            "invalid signature for {key}"
        );
        self.transaction.signatures[index] = signature;
        Ok(())
    }

    pub(crate) fn merge(&mut self, other: &Self) -> Result<()> {
        ensure!(
            self.transaction.message == other.transaction.message,
            "transactions do not match"
        );
        for (current, incoming) in self
            .transaction
            .signatures
            .iter()
            .zip(&other.transaction.signatures)
        {
            ensure!(
                *current == Signature::default()
                    || *incoming == Signature::default()
                    || current == incoming,
                "conflicting signatures"
            );
        }
        for (current, incoming) in self
            .transaction
            .signatures
            .iter_mut()
            .zip(&other.transaction.signatures)
        {
            if *incoming != Signature::default() {
                *current = *incoming;
            }
        }
        Ok(())
    }

    pub(crate) fn relay(
        &self,
        payer: &dyn Signer,
        extra_signers: &[&dyn Signer],
        blockhash: Hash,
    ) -> Result<VersionedTransaction> {
        ensure!(self.is_fully_signed(), "transaction is not fully signed");
        let required = self
            .inner
            .signer_keys()
            .into_iter()
            .filter(|key| self.signer_status().any(|(authority, _)| authority == *key))
            .collect::<Vec<_>>();
        let mut signers = vec![payer];
        let mut keys = vec![payer.try_pubkey()?];
        for signer in extra_signers {
            let key = signer.try_pubkey()?;
            if keys.contains(&key) {
                continue;
            }
            ensure!(
                required.contains(&&key),
                "outer signer {key} is not required"
            );
            keys.push(key);
            signers.push(*signer);
        }
        let mut instruction = submit(
            self.transaction.signatures.clone(),
            self.transaction.message.clone(),
        );
        for key in required {
            ensure!(keys.contains(key), "missing outer signer {key}");
            let meta = instruction
                .accounts
                .iter_mut()
                .find(|meta| meta.pubkey == *key)
                .context("missing relay signer account")?;
            meta.is_signer = true;
        }
        let message = VersionedMessage::Legacy(Message::new_with_blockhash(
            &[instruction],
            Some(&keys[0]),
            &blockhash,
        ));
        let relay = VersionedTransaction::try_new(message, &signers)?;
        let size = wincode::serialize(&relay)?.len();
        ensure!(
            size <= 1232,
            "relay transaction is {size} bytes; maximum is 1232"
        );
        Ok(relay)
    }
}

fn programmatic_signer(authority: &Address) -> Address {
    ProgrammaticSigner::derive_address(&spl_ed25519_signer_client::id(), authority)
}

fn validate_message(message: &Message) -> Result<()> {
    message.sanitize().context("invalid legacy message")?;
    ensure!(
        message.account_keys.len() <= 256,
        "too many message account keys"
    );
    ensure_unique(&message.account_keys)
}

fn ensure_unique(keys: &[Address]) -> Result<()> {
    for (index, key) in keys.iter().enumerate() {
        ensure!(
            !keys[index.saturating_add(1)..].contains(key),
            "duplicate message account {key}"
        );
    }
    Ok(())
}

fn is_writable(message: &Message, index: usize) -> bool {
    let signed = usize::from(message.header.num_required_signatures);
    if index < signed {
        index < signed.saturating_sub(usize::from(message.header.num_readonly_signed_accounts))
    } else {
        index
            < message
                .account_keys
                .len()
                .saturating_sub(usize::from(message.header.num_readonly_unsigned_accounts))
    }
}

#[cfg(test)]
mod tests;
