use {
    crate::{client::Client, output::OutputFormat, tx_config::TransactionConfigArgs},
    anyhow::{Context, Result, bail, ensure},
    base64::{Engine, prelude::BASE64_STANDARD},
    clap::{Args, ValueEnum, ValueHint},
    serde::Serialize,
    solana_account::Account,
    solana_address::Address,
    solana_cli_output::CliSignOnlyData,
    solana_clock::Clock,
    solana_message::{VersionedMessage, legacy::Message, v1},
    solana_nonce::{
        state::{Data, State},
        versions::Versions,
    },
    solana_rpc_client::nonblocking::rpc_client::RpcClient,
    solana_signature::Signature,
    solana_stake_interface::state::{Meta, StakeAuthorize, StakeStateV2},
    solana_system_interface::instruction::advance_nonce_account,
    solana_transaction::versioned::VersionedTransaction,
    spl_ed25519_signer_client::ProgrammaticSigner,
    spl_nonce_interface::state::Nonce,
    std::{fmt, fs, path::PathBuf},
};

// Covers two approval verifications and both stake authority changes with headroom.
const DEFAULT_COMPUTE_UNIT_LIMIT: u32 = 400_000;
// Match the legacy transaction default of 64 MiB for loaded account data.
const DEFAULT_LOADED_ACCOUNTS_DATA_SIZE_LIMIT: u32 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, ValueEnum)]
enum StakeAuthority {
    Staker,
    Withdrawer,
    Both,
}

impl StakeAuthority {
    fn authorizations(self) -> &'static [StakeAuthorize] {
        match self {
            Self::Staker => &[StakeAuthorize::Staker],
            Self::Withdrawer => &[StakeAuthorize::Withdrawer],
            Self::Both => &[StakeAuthorize::Staker, StakeAuthorize::Withdrawer],
        }
    }
}

#[derive(Debug, Args)]
pub struct StakeCommand {
    /// The single stake account to migrate.
    stake_account: Address,
    /// Current authority address. The new authority is its canonical programmatic signer PDA.
    #[clap(long)]
    authority: Address,
    /// Selected fields must each currently equal --authority.
    #[clap(long, value_enum)]
    authority_type: StakeAuthority,
    /// Existing System Program durable nonce account.
    #[clap(long)]
    legacy_nonce: Address,
    /// Existing SPL Nonce account controlled by the programmatic signer PDA.
    #[clap(long)]
    programmatic_nonce: Address,
    // Include the shared compute limit and priority fee options in this command.
    #[clap(flatten)]
    transaction_config: TransactionConfigArgs,
    /// Write the prepared migration as Solana CLI sign-only JSON for offline signing.
    #[clap(long, value_hint = ValueHint::FilePath)]
    outfile: PathBuf,
}

pub async fn run(command: StakeCommand, client: &Client, output: OutputFormat) -> Result<String> {
    // Validation on the current state of all accounts
    let snapshot = StakeMigrationState::read(
        client.rpc(),
        &command.stake_account,
        &command.legacy_nonce,
        &command.programmatic_nonce,
    )
    .await?;

    let fee_payer = client.fee_payer_address()?;
    let transaction = command.prepare_transaction(&snapshot, &fee_payer)?;
    let output_data = CliSignOnlyData {
        blockhash: transaction.message.recent_blockhash().to_string(),
        message: Some(BASE64_STANDARD.encode(transaction.message.serialize())),
        absent: transaction.message.static_account_keys()[..transaction.signatures.len()]
            .iter()
            .map(ToString::to_string)
            .collect(),
        ..CliSignOnlyData::default()
    };
    let json =
        serde_json::to_string_pretty(&output_data).context("failed to encode migration JSON")?;
    fs::write(&command.outfile, format!("{json}\n"))
        .with_context(|| format!("failed to write {}", command.outfile.display()))?;

    output.render(&StakePrepareOutput {
        path: command.outfile,
        stake_account: command.stake_account.to_string(),
        new_authority: ProgrammaticSigner::derive_address(
            &spl_ed25519_signer_client::id(),
            &command.authority,
        )
        .to_string(),
        required_signers: output_data.absent,
    })
}

impl StakeCommand {
    fn prepare_transaction(
        &self,
        snapshot: &StakeMigrationState,
        fee_payer: &Address,
    ) -> Result<VersionedTransaction> {
        let new_authority =
            ProgrammaticSigner::derive_address(&spl_ed25519_signer_client::id(), &self.authority);
        snapshot.validate_authorities(&self.authority, self.authority_type, &new_authority)?;

        // Changing the withdraw authority during an active lockup also requires custodian approval
        let custodian = (self
            .authority_type
            .authorizations()
            .contains(&StakeAuthorize::Withdrawer)
            && snapshot.meta.lockup.is_in_force(&snapshot.clock, None))
        .then_some(snapshot.meta.lockup.custodian);

        let instructions: Vec<_> = self
            .authority_type
            .authorizations()
            .iter()
            .map(|role| {
                solana_stake_interface::instruction::authorize_checked(
                    &self.stake_account,
                    &self.authority,
                    &new_authority,
                    *role,
                    match role {
                        StakeAuthorize::Withdrawer => custodian.as_ref(),
                        StakeAuthorize::Staker => None,
                    },
                )
            })
            .collect();
        let inner_msg = Message::new_with_blockhash(
            &instructions,
            Some(&new_authority),
            &snapshot.programmatic_nonce.nonce,
        );
        let executor_ix = spl_legacy_message_executor_client::instruction::execute(
            &self.programmatic_nonce,
            &inner_msg,
        );
        let mut authorities = vec![self.authority];
        if let Some(custodian) = custodian {
            if !authorities.contains(&custodian) {
                authorities.push(custodian);
            }
        }
        let execute_msg =
            spl_ed25519_signer_client::message::wrapped_message(&executor_ix, &authorities);

        // Required to collect all approvals before signing the outer transaction.
        // Inserting an approval changes the outer message and invalidates any existing outer signatures.
        let submit = spl_ed25519_signer_client::instruction::submit_with_outer_signers(
            vec![Signature::default(); authorities.len()],
            execute_msg,
            &authorities,
        );
        let args = &self.transaction_config;
        let config = v1::TransactionConfig {
            compute_unit_limit: Some(
                args.compute_unit_limit
                    .unwrap_or(DEFAULT_COMPUTE_UNIT_LIMIT),
            ),
            priority_fee: args.priority_fee,
            loaded_accounts_data_size_limit: Some(DEFAULT_LOADED_ACCOUNTS_DATA_SIZE_LIMIT),
            ..v1::TransactionConfig::empty()
        };
        let message = v1::Message::try_compile_with_config(
            fee_payer,
            &[
                advance_nonce_account(&self.legacy_nonce, &snapshot.legacy_nonce.authority),
                submit,
            ],
            snapshot.legacy_nonce.blockhash(),
            config,
        )?;
        Ok(VersionedTransaction {
            signatures: vec![
                Signature::default();
                usize::from(message.header.num_required_signatures)
            ],
            message: VersionedMessage::V1(message),
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StakePrepareOutput {
    path: PathBuf,
    stake_account: String,
    new_authority: String,
    required_signers: Vec<String>,
}

impl fmt::Display for StakePrepareOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Saved migration: {}", self.path.display())?;
        writeln!(formatter, "Stake account: {}", self.stake_account)?;
        writeln!(formatter, "New authority: {}", self.new_authority)?;
        write!(
            formatter,
            "Required signers: {}",
            self.required_signers.join(", ")
        )
    }
}

#[derive(Debug)]
struct StakeMigrationState {
    meta: Meta,
    legacy_nonce: Data,
    programmatic_nonce: Nonce,
    clock: Clock,
}

impl StakeMigrationState {
    async fn read(
        rpc: &RpcClient,
        stake: &Address,
        legacy_nonce: &Address,
        programmatic_nonce: &Address,
    ) -> Result<Self> {
        let accounts = rpc
            .get_multiple_accounts(&[
                *stake,
                *legacy_nonce,
                *programmatic_nonce,
                solana_sdk_ids::sysvar::clock::id(),
            ])
            .await
            .context("failed to fetch account snapshot")?;
        let [
            Some(stake),
            Some(legacy_nonce),
            Some(programmatic_nonce),
            Some(clock),
        ] = accounts.as_slice()
        else {
            bail!("an account was not found");
        };
        Self::decode(stake, legacy_nonce, programmatic_nonce, clock)
    }

    fn decode(
        stake: &Account,
        legacy_nonce: &Account,
        programmatic_nonce: &Account,
        clock: &Account,
    ) -> Result<Self> {
        ensure!(
            stake.owner == solana_stake_interface::program::id(),
            "account is not owned by the Stake program"
        );
        let stake_state: StakeStateV2 =
            wincode::deserialize(&stake.data).context("invalid stake account data")?;
        let meta = stake_state
            .meta()
            .context("stake account is not initialized")?;

        ensure!(
            legacy_nonce.owner == solana_sdk_ids::system_program::id(),
            "--legacy-nonce must be a System Program durable nonce account"
        );
        let versions: Versions = wincode::deserialize(&legacy_nonce.data)
            .context("invalid System nonce account data")?;
        ensure!(
            matches!(&versions, Versions::Current(_)),
            "System nonce account must use the current nonce version"
        );
        let State::Initialized(legacy_nonce) = versions.state() else {
            bail!("System nonce account is not initialized");
        };

        ensure!(
            programmatic_nonce.owner == spl_nonce_interface::id(),
            "--programmatic-nonce must be an SPL Nonce account"
        );
        let programmatic_nonce = Nonce::view(&programmatic_nonce.data)
            .cloned()
            .context("invalid SPL Nonce account data")?;
        ensure!(
            clock.owner == solana_sdk_ids::sysvar::id(),
            "invalid Clock sysvar owner"
        );

        let clock = wincode::deserialize(&clock.data).context("invalid Clock sysvar")?;
        Ok(Self {
            meta,
            legacy_nonce: legacy_nonce.clone(),
            programmatic_nonce,
            clock,
        })
    }

    fn validate_authorities(
        &self,
        authority: &Address,
        selection: StakeAuthority,
        new_authority: &Address,
    ) -> Result<()> {
        for authorization in selection.authorizations() {
            let (current, role) = match authorization {
                StakeAuthorize::Staker => (self.meta.authorized.staker, "staker"),
                StakeAuthorize::Withdrawer => (self.meta.authorized.withdrawer, "withdrawer"),
            };
            ensure!(
                current == *authority,
                "{role} authority does not match --authority"
            );
        }
        ensure!(
            self.programmatic_nonce.authority == *new_authority,
            "SPL nonce authority is not the new authority"
        );
        Ok(())
    }
}
