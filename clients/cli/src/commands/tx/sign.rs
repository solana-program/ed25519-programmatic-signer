use {
    super::sign_only_data::CliSignOnlyDataExt,
    crate::{client::Client, output::OutputFormat},
    anyhow::{Context, Result, ensure},
    clap::{Args, ValueHint},
    gag::Redirect,
    solana_address::Address,
    solana_cli_output::CliSignOnlyData,
    solana_message::VersionedMessage,
    solana_sanitize::Sanitize,
    solana_signature::Signature,
    solana_signer::Signer,
    spl_ed25519_signer_client::ProgrammaticSigner,
    spl_legacy_message_executor_interface::instruction::Instruction as ExecutorInstruction,
    std::{
        collections::{BTreeMap, BTreeSet},
        fmt::Write as _,
        io::{self, Write},
        path::PathBuf,
    },
};

#[derive(Debug, Args)]
pub(super) struct SignCommand {
    /// Solana CLI sign-only JSON (`CliSignOnlyData`) with a base64 message containing
    /// exactly one Execute instruction.
    /// Supports legacy, v0, and v1 approvals using static executor accounts.
    #[clap(value_name = "SIGN_ONLY_FILE", value_hint = ValueHint::FilePath)]
    sign_only_file: PathBuf,

    /// Write updated `CliSignOnlyData` JSON, including the message, to a new file.
    #[clap(long, value_name = "FILEPATH", value_hint = ValueHint::FilePath)]
    outfile: PathBuf,
}

pub(super) fn run(command: SignCommand, client: &Client, output: OutputFormat) -> Result<String> {
    let mut sign_only_data = CliSignOnlyData::read(&command.sign_only_file)?;
    let message = sign_only_data.deserialize_message()?;
    let approvals = sign_only_data.verified_signatures()?;

    let summary = build_signing_summary(&message, &approvals)?;
    eprintln!("{summary}");
    let (address, signature) = with_stdout_redirected_to_stderr(|| {
        let signer = client.default_signer("approval authority")?;
        sign_approval_message(&message, signer.as_ref()).context("failed to sign approval message")
    })?;
    sign_only_data.add_signature(address, signature)?;
    let rendered = output.render(&sign_only_data)?;
    sign_only_data.write_new(&command.outfile)?;
    Ok(rendered)
}

/// Sign the message without reconstructing a transaction or modifying other approvals.
fn sign_approval_message(
    message: &VersionedMessage,
    signer: &dyn Signer,
) -> Result<(Address, Signature)> {
    let address = signer.try_pubkey()?;
    let signature = signer.try_sign_message(&message.serialize())?;
    Ok((address, signature))
}

/// Check the message's structure and build an offline signing summary, not execution preflight.
fn build_signing_summary(
    approval_message: &VersionedMessage,
    approvals: &BTreeMap<Address, Signature>,
) -> Result<String> {
    let account_keys = approval_message.static_account_keys();
    // This command accepts Execute approval messages, not arbitrary instructions or Submit.
    let [instruction] = approval_message.instructions() else {
        anyhow::bail!("expected exactly one Execute instruction");
    };
    ensure!(
        account_keys.get(usize::from(instruction.program_id_index))
            == Some(&spl_legacy_message_executor_interface::id()),
        "expected the Legacy Message Executor. Other programs and Submit relays are not accepted"
    );
    // Like Submit, resolve only static accounts. Unused v0 lookups are allowed.
    ensure!(
        instruction
            .accounts
            .iter()
            .all(|index| usize::from(*index) < account_keys.len()),
        "executor accounts must use static account keys. Address table lookups are not resolved"
    );
    let ExecutorInstruction::Execute(executed_message) =
        ExecutorInstruction::try_from_bytes(&instruction.data)
            .context("invalid Execute instruction")?;
    // Validate the embedded message before displaying its account flags and indexed programs.
    executed_message
        .sanitize()
        .context("invalid executed message")?;
    let nonce_index = instruction
        .accounts
        .first()
        .context("missing nonce account")?;
    let nonce_account = account_keys[usize::from(*nonce_index)];

    let mut output = String::new();
    writeln!(
        output,
        "Signing the supplied Execute message. No separate Submit relay is constructed or signed, \
         and nothing is submitted to the network."
    )?;
    writeln!(output, "Message hash (BLAKE3): {}", approval_message.hash())?;
    writeln!(
        output,
        "Blockhash (opaque signed field): {}",
        approval_message.recent_blockhash()
    )?;
    writeln!(output, "SPL nonce account: {nonce_account}")?;
    writeln!(
        output,
        "SPL nonce value: {}",
        executed_message.recent_blockhash
    )?;
    writeln!(output, "Approval authorities:")?;
    for authority_address in account_keys.iter().take(usize::from(
        approval_message.header().num_required_signatures,
    )) {
        let programmatic_signer_address =
            ProgrammaticSigner::derive_address(&spl_ed25519_signer_client::id(), authority_address);
        let status = if approvals.contains_key(authority_address) {
            "present (verified)"
        } else {
            "missing"
        };
        writeln!(
            output,
            "  {authority_address}: {status}, programmatic signer {programmatic_signer_address}"
        )?;
    }
    writeln!(
        output,
        "Executed message accounts (signer requirements, not outer signatures):"
    )?;
    for (index, key) in executed_message.account_keys.iter().enumerate() {
        writeln!(
            output,
            "  [{index}] {key}, signer={}, writable={}",
            executed_message.is_signer(index),
            executed_message.is_maybe_writable_with_reserved_addresses(index, None::<&BTreeSet<_>>)
        )?;
    }
    writeln!(
        output,
        "Executed instructions (data shown as hex, not decoded):"
    )?;
    for (index, instruction) in executed_message.instructions.iter().enumerate() {
        writeln!(
            output,
            "  [{index}] program={}, accounts={:?}, data={:02x?}",
            instruction.program_id(&executed_message.account_keys),
            instruction.accounts,
            instruction.data
        )?;
    }
    write!(
        output,
        "Offline review: execution validity, cluster identity, live nonce state and business \
         intent are not verified."
    )?;
    Ok(output)
}

/// Keep wallet loading and approval progress out of the command's output document.
/// Remove this interception when the wallet SDK routes approval diagnostics to stderr.
///
/// Redirection is process-wide: use only at the synchronous CLI signing seam, never in the
/// reusable signing code. The reentrant lock makes unrelated Rust stdout writers wait.
/// It cannot coordinate raw descriptor writes or a signer waiting on another stdout writer.
fn with_stdout_redirected_to_stderr<T>(operation: impl FnOnce() -> Result<T>) -> Result<T> {
    let mut stdout = io::stdout().lock();
    stdout.flush().context("failed to flush command output")?;
    let redirect =
        Redirect::stdout(io::stderr()).context("failed to redirect wallet progress to stderr")?;
    let result = operation();
    // Flush buffered prompts before restoring stdout, on signing and loading errors too.
    let flush_result = stdout.flush();
    drop(redirect);
    let value = result?;
    flush_result.context("failed to flush wallet progress")?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use {
        super::{sign_approval_message, with_stdout_redirected_to_stderr},
        solana_address::Address,
        solana_keypair::Keypair,
        solana_message::{VersionedMessage, legacy::Message},
        solana_signature::Signature,
        solana_signer::{Signer, SignerError},
        std::{cell::Cell, process::Command},
    };

    #[test]
    fn signing_errors_return_no_approval() {
        struct FailingSigner {
            address: Address,
            failure: &'static str,
            signing_calls: Cell<usize>,
        }

        impl Signer for FailingSigner {
            fn try_pubkey(&self) -> Result<Address, SignerError> {
                if self.failure == "address" {
                    Err(SignerError::Custom("cannot read address".to_string()))
                } else {
                    Ok(self.address)
                }
            }

            fn try_sign_message(&self, _: &[u8]) -> Result<Signature, SignerError> {
                self.signing_calls
                    .set(self.signing_calls.get().saturating_add(1));
                Err(SignerError::UserCancel("rejected".to_string()))
            }

            fn is_interactive(&self) -> bool {
                false
            }
        }

        let message = VersionedMessage::Legacy(Message::default());
        for (failure, expected_error, expected_calls) in [
            ("address", "cannot read address", 0),
            ("signing", "rejected", 1),
        ] {
            let signer = FailingSigner {
                address: Address::new_unique(),
                failure,
                signing_calls: Cell::new(0),
            };
            let error = sign_approval_message(&message, &signer).unwrap_err();
            assert!(
                error.to_string().contains(expected_error),
                "{failure}: {error}"
            );
            assert_eq!(signer.signing_calls.get(), expected_calls, "{failure}");
        }
    }

    #[test]
    fn signs_the_serialized_message_exactly_once() {
        struct RecordingSigner {
            keypair: Keypair,
            signing_calls: Cell<usize>,
            expected_bytes: Vec<u8>,
        }

        impl Signer for RecordingSigner {
            fn try_pubkey(&self) -> Result<Address, SignerError> {
                self.keypair.try_pubkey()
            }

            fn try_sign_message(&self, message: &[u8]) -> Result<Signature, SignerError> {
                assert_eq!(message, self.expected_bytes);
                self.signing_calls
                    .set(self.signing_calls.get().saturating_add(1));
                self.keypair.try_sign_message(message)
            }

            fn is_interactive(&self) -> bool {
                false
            }
        }

        let keypair = Keypair::new();
        let message = VersionedMessage::Legacy(Message::new(&[], Some(&keypair.pubkey())));
        let message_bytes = message.serialize();
        let signer = RecordingSigner {
            keypair,
            signing_calls: Cell::new(0),
            expected_bytes: message_bytes.clone(),
        };
        let (address, signature) = sign_approval_message(&message, &signer).unwrap();
        assert_eq!(address, signer.keypair.pubkey());
        assert!(signature.verify(address.as_ref(), &message_bytes));
        assert_eq!(signer.signing_calls.get(), 1);
    }

    #[test]
    fn redirects_progress_and_restores_stdout_on_success_and_errors() {
        const CHILD_CASE: &str = "PSIGNER_TEST_WALLET_OUTPUT_CASE";
        const BEGIN: &str = "BEGIN_OUTPUT\n";
        const END: &str = "END_OUTPUT";
        if let Ok(outcome) = std::env::var(CHILD_CASE) {
            print!("{BEGIN}");
            let result = with_stdout_redirected_to_stderr(|| {
                println!("Loading wallet");
                anyhow::ensure!(outcome != "load-error", "wallet unavailable");
                println!("Waiting for your approval");
                // Match the SDK's buffered stdout progress, including its final partial line.
                if outcome == "sign-error" {
                    print!("Rejected");
                    anyhow::bail!("wallet refused");
                }
                print!("Approved");
                Ok(())
            });
            match result {
                Ok(()) => println!("{{\"ok\":true}}"),
                Err(error) => eprintln!("{error:#}"),
            }
            println!("{END}");
            return;
        }

        // Run without libtest capture in an isolated child because redirection is process-wide.
        for outcome in ["success", "load-error", "sign-error"] {
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "commands::tx::sign::tests::redirects_progress_and_restores_stdout_on_success_and_errors",
                    "--nocapture",
                ])
                .env(CHILD_CASE, outcome)
                .output()
                .unwrap();
            let stdout = String::from_utf8(output.stdout).unwrap();
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert!(
                output.status.success(),
                "stdout: {stdout}\nstderr: {stderr}"
            );
            let command_output = stdout
                .split_once(BEGIN)
                .unwrap()
                .1
                .split_once(END)
                .unwrap()
                .0;
            assert!(!stdout.contains("Loading wallet"), "{stdout}");
            assert!(!stdout.contains("Waiting for your approval"), "{stdout}");
            assert!(stderr.contains("Loading wallet"), "{stderr}");
            match outcome {
                "success" => {
                    assert_eq!(command_output, "{\"ok\":true}\n");
                    assert!(stderr.contains("Waiting for your approval"), "{stderr}");
                    assert!(stderr.contains("Approved"), "{stderr}");
                }
                "load-error" => {
                    assert!(command_output.is_empty(), "{stdout}");
                    assert!(stderr.contains("wallet unavailable"), "{stderr}");
                }
                "sign-error" => {
                    assert!(command_output.is_empty(), "{stdout}");
                    assert!(stderr.contains("Rejected"), "{stderr}");
                    assert!(stderr.contains("wallet refused"), "{stderr}");
                }
                _ => unreachable!(),
            }
        }
    }
}
