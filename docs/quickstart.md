# Quickstart

This walkthrough teaches you to prepare a SOL transfer, inspect and sign it offline,
then submit it with an online fee payer. Run steps 1–7 in order. After that, choose
from the optional recipes for tokens, multiple signatures, designated relayers,
nonce chains, cancellation, and other program instructions.

All transactions here use a fresh local validator running this checkout's programs.
The commands and program IDs below are not a guide to a public-network deployment.

## Before you start

Use Bash in two terminals, both opened at the repository root. You need:

- Rust from `rust-toolchain.toml` and `nightly-2026-01-22` for the CLI build. If the
  nightly is missing, install it with `rustup toolchain install nightly-2026-01-22`.
- Solana CLI **3.1.8**, including `solana-keygen`, `solana-test-validator`, and the
  `cargo build-sbf` tools.
- `make` and `jq`. The token recipe additionally needs SPL Token CLI **5.5.0**.

Three roles participate. You will play all three on one machine using disposable
keys, while keeping their responsibilities distinct:

| Role | What it does | Key or account |
| --- | --- | --- |
| Coordinator | Builds a transaction file from instructions and the current nonce | Only needs the cold authority's public address |
| Cold signer | Reviews and signs the file; needs no RPC or SOL | The cold authority key |
| Relayer | Submits the signed file and pays the network fee | A funded online key |

The **programmatic signer PDA** is the address that owns and spends the assets.
It is derived from the cold authority, but has no private key. The **SPL Nonce
account** holds a value that changes when a transaction succeeds, preventing replay.
See [how it works](architecture.md) for the program execution path.

## 1. Build and start the local validator — terminal A

Build the CLI and the three programs:

```sh
make build-clients-cli build-sbf-nonce-program build-sbf-signer-program build-sbf-executor-program
```

Start a validator with a fresh ledger and the programs loaded at their declared IDs:

```sh
solana-test-validator --ledger "$(mktemp -d "$PWD/target/quickstart-ledger.XXXXXX")" \
  --bind-address 127.0.0.1 --rpc-port 18899 --faucet-port 18901 \
  --bpf-program Noncediea1fH12usShuQAz28UhgAeuE5Maf32LsMUQB target/deploy/spl_nonce_program.so \
  --bpf-program EdSigVfK1DkeMrjFNDMjwfQaJPhPTtX7jW8uPv3oKEgN target/deploy/spl_ed25519_signer_program.so \
  --bpf-program ExecxgyHYsAXB4c5dZodV1zJZ9hqfsDCYkRDRATrpkFR target/deploy/spl_legacy_message_executor_program.so
```

Wait until processed slots advance, then leave this terminal running. Ports
18899–18901 must be available. If you choose different ports, use the matching RPC
URL in step 2. Run all remaining commands in terminal B.

## 2. Create keys and an isolated CLI configuration — terminal B

Create a new working directory and three local keypair files:

```sh
CLI="$PWD/target/debug/spl-programmatic-signer-cli"
WORK=$(mktemp -d "$PWD/target/quickstart.XXXXXX")
RPC=http://127.0.0.1:18899
for key in payer cold recipient; do
  solana-keygen new --silent --no-bip39-passphrase --outfile "$WORK/$key.json"
done
PAYER="$WORK/payer.json"
COLD="$WORK/cold.json"
CONFIG="$WORK/config.yml"
```

Set the RPC, online payer, and confirmation level in a configuration used only by
this walkthrough. `-C` selects this file; your default configuration is unchanged:

```sh
solana -C "$CONFIG" config set --url "$RPC" --keypair "$PAYER" --commitment confirmed
solana -C "$CONFIG" cluster-version
GENESIS_HASH=$(solana -C "$CONFIG" genesis-hash)
COLD_ADDRESS=$(solana -C "$CONFIG" address -k "$COLD")
RECIPIENT=$(solana-keygen pubkey "$WORK/recipient.json")
PDA=$("$CLI" -C "$CONFIG" address "$COLD_ADDRESS")
solana -C "$CONFIG" airdrop 10 "$(solana-keygen pubkey "$PAYER")"
solana -C "$CONFIG" balance
printf 'Cold authority: %s\nPDA: %s\nRecipient: %s\nGenesis hash: %s\n' \
  "$COLD_ADDRESS" "$PDA" "$RECIPIENT" "$GENESIS_HASH"
```

The payer balance should be **10 SOL**, and the PDA should differ from the cold
address. The local faucet funds the payer; it does not fund the cold authority.
For actual offline use, the cold key stays on the signing machine and only its
public address is given to the coordinator.

## 3. Create a nonce and fund the PDA

Create an SPL Nonce account controlled by the cold authority's PDA. The command
generates the nonce account's creation keypair automatically:

```sh
"$CLI" -C "$CONFIG" --output json-compact nonce create \
  --cold-authority "$COLD_ADDRESS" > "$WORK/nonce.json"
NONCE_ACCOUNT=$(jq -er .nonceAccount "$WORK/nonce.json")
NONCE_VALUE=$(jq -er .nonce "$WORK/nonce.json")
"$CLI" -C "$CONFIG" nonce show "$NONCE_ACCOUNT"
solana -C "$CONFIG" transfer "$PDA" 0.1 --allow-unfunded-recipient
solana -C "$CONFIG" balance "$PDA"
```

The nonce's authority should match `PDA`, and the PDA balance should be **0.1 SOL**.
These are the assets the inner transfer will spend. The online payer separately
pays for account creation and network fees.

## 4. Prepare the transfer without signing or submitting it

Use the stock Solana CLI to build a transfer from the PDA to the recipient:

```sh
solana -C "$CONFIG" transfer "$RECIPIENT" 0.001 \
  --from "$PDA" --fee-payer "$PDA" --blockhash "$NONCE_VALUE" \
  --sign-only --dump-transaction-message --output json-compact \
  --allow-unfunded-recipient > "$WORK/transfer.source.json"
jq '{blockhash, absent}' "$WORK/transfer.source.json"
```

The output should list the PDA as an absent signer and show `NONCE_VALUE` as the
blockhash. This is expected: a PDA cannot sign with a keypair. Here `--blockhash`
carries the SPL nonce value; do not use Solana's native `--nonce` option, which
uses a different protocol. `--fee-payer "$PDA"` identifies an inner signer; the
online payer will pay the actual network fee when relaying.

Wrap the source message into the transaction file that the cold authority signs:

```sh
"$CLI" -C "$CONFIG" transaction create --from-sign-only "$WORK/transfer.source.json" \
  --nonce "$NONCE_ACCOUNT" --authority "$COLD_ADDRESS" --fetch-nonce \
  --outfile "$WORK/transfer.json"
"$CLI" -C "$CONFIG" transaction simulate inner "$WORK/transfer.json"
```

Expect `inner simulation succeeded`. This checks the transfer against current
accounts without consuming the nonce or requiring cold signatures. It is a preview;
state can still change before submission.

Keep the three transaction files distinct:

| File | Contents | Next user |
| --- | --- | --- |
| `transfer.source.json` | Instructions and nonce from the stock CLI's sign-only output | Coordinator |
| `transfer.json` | Wrapped transaction with empty signature slots | Cold signer |
| `transfer.signed.json` | The same wrapped message with signatures added in step 5 | Relayer |

These files contain no private keys. Wrapped files use JSON serialization of
`solana_transaction::Transaction`; signatures cover the binary message, so changing
JSON whitespace does not invalidate them. These wrapped files are inputs to
`transaction submit`, which builds the live network transaction.

## 5. Inspect, then sign offline

Run these commands in terminal B for this local exercise. With separate machines,
move `transfer.json` to the signing machine, adjust the file paths, and inspect it
there. `inspect` and `sign` do not query RPC; a separate signing machine can omit
`-C "$CONFIG"` and use its own cold key.

```sh
"$CLI" -C "$CONFIG" transaction inspect "$WORK/transfer.json"
```

Before signing, check that inspection shows:

- A transfer of **1,000,000 lamports** from `PDA` to `RECIPIENT`.
- `NONCE_ACCOUNT` and `NONCE_VALUE` as the nonce account and expected nonce.
- The same genesis hash recorded in `GENESIS_HASH` during setup.
- `COLD_ADDRESS` as the transaction signer, currently marked missing.

The predicted next nonce is what this exact inner message will produce if it
succeeds. Signing changes only the signature slots:

```sh
"$CLI" -C "$CONFIG" transaction sign "$WORK/transfer.json" --keypair "$COLD" \
  --outfile "$WORK/transfer.signed.json"
"$CLI" -C "$CONFIG" transaction inspect "$WORK/transfer.signed.json"
```

The authority should now be marked signed, with the same transfer and nonce.
Return only the signed transaction file to the relayer; keep the cold key offline.

## 6. Verify, simulate the relay, and submit

The online relayer checks the signed file against the live nonce and cluster:

```sh
"$CLI" -C "$CONFIG" transaction verify "$WORK/transfer.signed.json" --fetch-nonce
"$CLI" -C "$CONFIG" transaction simulate relay "$WORK/transfer.signed.json"
```

Expect `Fully signed: true` and `relay simulation succeeded`. Relay simulation
checks the complete programmatic signing and nonce execution path without sending
it. Now submit with the online payer selected by the configuration:

```sh
"$CLI" -C "$CONFIG" transaction submit "$WORK/transfer.signed.json"
solana -C "$CONFIG" balance "$RECIPIENT" --lamports
"$CLI" -C "$CONFIG" nonce show "$NONCE_ACCOUNT"
```

Submission prints a confirmed signature and predicted/observed successor nonce.
The recipient should hold **1,000,000 lamports**. The stored nonce should have
changed to the predicted successor; the original file no longer matches it.

## 7. Check replay protection

Submit the same file again. **This command is expected to fail** with
`nonce mismatch`; continue after that error. A different error needs investigation.

```sh
"$CLI" -C "$CONFIG" transaction submit "$WORK/transfer.signed.json"
```

The transfer cannot land twice. To send another transaction, build a new source
message using the current nonce, or prepare a chain as below.

## Optional recipes

These recipes reuse terminal B's keys, configuration, funded PDA, recipient, and
nonce account after step 7. Each reads the nonce it needs, so you may choose them
independently. Run each recipe once in this working directory; use new filenames
if you repeat one. Keep the validator running until you finish.

### Pre-sign a chain and sign a batch

Continue in the same session. Read the current nonce and prepare the first file:

```sh
NONCE_VALUE=$("$CLI" -C "$CONFIG" --output json-compact nonce show "$NONCE_ACCOUNT" | jq -er .nonce)
solana -C "$CONFIG" transfer "$RECIPIENT" 0.001 \
  --from "$PDA" --fee-payer "$PDA" --blockhash "$NONCE_VALUE" \
  --sign-only --dump-transaction-message --output json-compact > "$WORK/first.source.json"
"$CLI" -C "$CONFIG" transaction create --from-sign-only "$WORK/first.source.json" \
  --nonce "$NONCE_ACCOUNT" --authority "$COLD_ADDRESS" --fetch-nonce --outfile "$WORK/first.json"
```

`inspect` includes `nextNonce`, conditional on that exact message succeeding.
Use it to build the second file before submitting the first. `--after` checks
the successor against its predecessor and supplies the genesis hash without RPC:

```sh
"$CLI" -C "$CONFIG" transaction inspect "$WORK/first.json"
NEXT=$("$CLI" -C "$CONFIG" --output json-compact transaction inspect "$WORK/first.json" | jq -er .nextNonce)
solana -C "$CONFIG" transfer "$RECIPIENT" 0.001 \
  --from "$PDA" --fee-payer "$PDA" --blockhash "$NEXT" \
  --sign-only --dump-transaction-message --output json-compact > "$WORK/second.source.json"
"$CLI" -C "$CONFIG" transaction create --from-sign-only "$WORK/second.source.json" \
  --nonce "$NONCE_ACCOUNT" --authority "$COLD_ADDRESS" --after "$WORK/first.json" \
  --outfile "$WORK/second.json"
"$CLI" -C "$CONFIG" transaction inspect "$WORK/second.json"
mkdir "$WORK/signed"
"$CLI" -C "$CONFIG" transaction sign "$WORK/first.json" "$WORK/second.json" \
  --keypair "$COLD" --outdir "$WORK/signed"
```

**Expected failure:** submitting the second file first returns `nonce mismatch`.
Continue after that error:

```sh
"$CLI" -C "$CONFIG" transaction submit "$WORK/signed/second.json"
```

Submit in order. A competing transition would invalidate the planned descendants.
Use separate nonce accounts when transactions must proceed independently.

```sh
"$CLI" -C "$CONFIG" transaction submit "$WORK/signed/first.json"
"$CLI" -C "$CONFIG" transaction submit "$WORK/signed/second.json"
```

### Cancel a pending file

Prepare and sign another transfer, but leave it unsubmitted:

```sh
NONCE_VALUE=$("$CLI" -C "$CONFIG" --output json-compact nonce show "$NONCE_ACCOUNT" | jq -er .nonce)
solana -C "$CONFIG" transfer "$RECIPIENT" 0.001 \
  --from "$PDA" --fee-payer "$PDA" --blockhash "$NONCE_VALUE" \
  --sign-only --dump-transaction-message --output json-compact > "$WORK/pending.source.json"
"$CLI" -C "$CONFIG" transaction create --from-sign-only "$WORK/pending.source.json" \
  --nonce "$NONCE_ACCOUNT" --authority "$COLD_ADDRESS" --fetch-nonce --outfile "$WORK/pending.json"
"$CLI" -C "$CONFIG" transaction inspect "$WORK/pending.json"
"$CLI" -C "$CONFIG" transaction sign "$WORK/pending.json" --keypair "$COLD" --outfile "$WORK/pending.signed.json"
```

Build an empty transaction that consumes the same nonce. Inspection should show no
inner instructions and the same expected nonce as the pending file. Sign and submit:

```sh
"$CLI" -C "$CONFIG" nonce advance --from-transaction "$WORK/pending.json" \
  --authority "$COLD_ADDRESS" --outfile "$WORK/cancel.json"
"$CLI" -C "$CONFIG" transaction inspect "$WORK/cancel.json"
"$CLI" -C "$CONFIG" transaction sign "$WORK/cancel.json" --keypair "$COLD" --outfile "$WORK/cancel.signed.json"
"$CLI" -C "$CONFIG" transaction submit "$WORK/cancel.signed.json"
```

Cancellation takes effect only when it lands. The following command is expected
to fail with `nonce mismatch`; continue after that error. If the pending transfer
had landed first, cancellation would fail instead.

```sh
"$CLI" -C "$CONFIG" transaction submit "$WORK/pending.signed.json"
```

### Collect multiple cold signatures

Add a second approval authority. Every listed authority must sign this file; the
transfer still spends from the first authority's PDA. The nonce account's authority
remains unchanged:

```sh
solana-keygen new --silent --no-bip39-passphrase --outfile "$WORK/cold-2.json"
COLD_ADDRESS_2=$(solana-keygen pubkey "$WORK/cold-2.json")
NONCE_VALUE=$("$CLI" -C "$CONFIG" --output json-compact nonce show "$NONCE_ACCOUNT" | jq -er .nonce)
solana -C "$CONFIG" transfer "$RECIPIENT" 0.001 \
  --from "$PDA" --fee-payer "$PDA" --blockhash "$NONCE_VALUE" \
  --sign-only --dump-transaction-message --output json-compact > "$WORK/multisig.source.json"
"$CLI" -C "$CONFIG" transaction create --from-sign-only "$WORK/multisig.source.json" \
  --nonce "$NONCE_ACCOUNT" --authority "$COLD_ADDRESS" --authority "$COLD_ADDRESS_2" \
  --fetch-nonce --outfile "$WORK/multisig.json"
"$CLI" -C "$CONFIG" transaction inspect "$WORK/multisig.json"
"$CLI" -C "$CONFIG" transaction sign "$WORK/multisig.json" --keypair "$COLD" \
  --outfile "$WORK/multisig.first.json"
"$CLI" -C "$CONFIG" transaction sign "$WORK/multisig.json" --keypair "$WORK/cold-2.json" \
  --outfile "$WORK/multisig.second.json"
"$CLI" -C "$CONFIG" transaction merge "$WORK/multisig.first.json" "$WORK/multisig.second.json" \
  --outfile "$WORK/multisig.signed.json"
"$CLI" -C "$CONFIG" transaction verify "$WORK/multisig.signed.json" --fetch-nonce
"$CLI" -C "$CONFIG" transaction simulate relay "$WORK/multisig.signed.json"
"$CLI" -C "$CONFIG" transaction submit "$WORK/multisig.signed.json"
```

Each authority independently inspects and signs a copy. Merge accepts only copies
of exactly the same message with valid signatures. Verification should report
`Fully signed: true` after merging both approvals.

### Require a designated relayer

Ordinarily anyone holding a fully signed file may relay it. To restrict that,
make a separate relayer key an inner signer and name it with `--submit-signer`.
Here it is the inner fee-payer slot; the funded online payer still pays network fees:

```sh
solana-keygen new --silent --no-bip39-passphrase --outfile "$WORK/relayer.json"
RELAYER=$(solana-keygen pubkey "$WORK/relayer.json")
NONCE_VALUE=$("$CLI" -C "$CONFIG" --output json-compact nonce show "$NONCE_ACCOUNT" | jq -er .nonce)
solana -C "$CONFIG" transfer "$RECIPIENT" 0.001 \
  --from "$PDA" --fee-payer "$RELAYER" --blockhash "$NONCE_VALUE" \
  --sign-only --dump-transaction-message --output json-compact > "$WORK/designated.source.json"
"$CLI" -C "$CONFIG" transaction create --from-sign-only "$WORK/designated.source.json" \
  --nonce "$NONCE_ACCOUNT" --authority "$COLD_ADDRESS" --submit-signer "$RELAYER" \
  --fetch-nonce --outfile "$WORK/designated.json"
"$CLI" -C "$CONFIG" transaction inspect "$WORK/designated.json"
"$CLI" -C "$CONFIG" transaction sign "$WORK/designated.json" --keypair "$COLD" \
  --outfile "$WORK/designated.cold.json"
"$CLI" -C "$CONFIG" transaction sign "$WORK/designated.cold.json" --keypair "$WORK/relayer.json" \
  --outfile "$WORK/designated.ready.json"
```

Without the live relayer key, simulation is expected to fail with `missing outer
signer` even though both file signatures are present:

```sh
"$CLI" -C "$CONFIG" transaction simulate relay "$WORK/designated.ready.json"
```

Supply that key to simulate and submit successfully:

```sh
"$CLI" -C "$CONFIG" transaction simulate relay "$WORK/designated.ready.json" \
  --submit-signer "$WORK/relayer.json"
"$CLI" -C "$CONFIG" transaction submit "$WORK/designated.ready.json" \
  --submit-signer "$WORK/relayer.json"
```

The relayer signs both the file and the live outer transaction. A different fee
payer cannot bypass the required relayer's live signature.

### Transfer SPL tokens

Create a local six-decimal mint and two token accounts, then mint two tokens to
the PDA. These setup transactions use the online payer:

```sh
for key in mint source-token destination-token; do
  solana-keygen new --silent --no-bip39-passphrase --outfile "$WORK/$key.json"
done
MINT=$(solana-keygen pubkey "$WORK/mint.json")
SOURCE_TOKEN=$(solana-keygen pubkey "$WORK/source-token.json")
DESTINATION_TOKEN=$(solana-keygen pubkey "$WORK/destination-token.json")
spl-token -u "$RPC" create-token "$WORK/mint.json" --decimals 6 \
  --fee-payer "$PAYER" --mint-authority "$PAYER"
spl-token -u "$RPC" create-account "$MINT" "$WORK/source-token.json" --owner "$PDA" --fee-payer "$PAYER"
spl-token -u "$RPC" create-account "$MINT" "$WORK/destination-token.json" --owner "$RECIPIENT" --fee-payer "$PAYER"
spl-token -u "$RPC" mint "$MINT" 2 "$SOURCE_TOKEN" --fee-payer "$PAYER" --mint-authority "$PAYER"
```

Build a checked transfer of 1.25 tokens, inspect the mint, amount and decimals,
then follow the same signing and relay flow:

```sh
NONCE_VALUE=$("$CLI" -C "$CONFIG" --output json-compact nonce show "$NONCE_ACCOUNT" | jq -er .nonce)
spl-token -u "$RPC" transfer "$MINT" 1.25 "$DESTINATION_TOKEN" --from "$SOURCE_TOKEN" \
  --owner "$PDA" --fee-payer "$PDA" --blockhash "$NONCE_VALUE" --mint-decimals 6 \
  --sign-only --dump-transaction-message --output json-compact > "$WORK/token.source.json"
"$CLI" -C "$CONFIG" transaction create --from-sign-only "$WORK/token.source.json" \
  --nonce "$NONCE_ACCOUNT" --authority "$COLD_ADDRESS" --fetch-nonce --outfile "$WORK/token.json"
"$CLI" -C "$CONFIG" transaction simulate inner "$WORK/token.json"
"$CLI" -C "$CONFIG" transaction inspect "$WORK/token.json"
"$CLI" -C "$CONFIG" transaction sign "$WORK/token.json" --keypair "$COLD" --outfile "$WORK/token.signed.json"
"$CLI" -C "$CONFIG" transaction verify "$WORK/token.signed.json" --fetch-nonce
"$CLI" -C "$CONFIG" transaction simulate relay "$WORK/token.signed.json"
"$CLI" -C "$CONFIG" transaction submit "$WORK/token.signed.json"
spl-token -u "$RPC" balance --address "$SOURCE_TOKEN"
spl-token -u "$RPC" balance --address "$DESTINATION_TOKEN"
```

The source holds 0.75 tokens and the recipient holds 1.25. A token-owning PDA
need not hold SOL; the online payer covers account creation and relay fees.

### Include other program instructions

The import path accepts a legacy message containing instructions for other programs.
As a runnable example, use the stock CLI to add a Memo instruction to a SOL transfer:

```sh
NONCE_VALUE=$("$CLI" -C "$CONFIG" --output json-compact nonce show "$NONCE_ACCOUNT" | jq -er .nonce)
solana -C "$CONFIG" transfer "$RECIPIENT" 0.001 \
  --from "$PDA" --fee-payer "$PDA" --blockhash "$NONCE_VALUE" \
  --with-memo 'CLI walkthrough memo' \
  --sign-only --dump-transaction-message --output json-compact > "$WORK/memo.source.json"
"$CLI" -C "$CONFIG" transaction create --from-sign-only "$WORK/memo.source.json" \
  --nonce "$NONCE_ACCOUNT" --authority "$COLD_ADDRESS" --fetch-nonce --outfile "$WORK/memo.json"
"$CLI" -C "$CONFIG" transaction inspect "$WORK/memo.json"
"$CLI" -C "$CONFIG" transaction simulate inner "$WORK/memo.json"
```

Inspection should show both the SOL transfer and the memo text. Both instructions
belong to the same signed message and execute in the same transaction:

```sh
"$CLI" -C "$CONFIG" transaction sign "$WORK/memo.json" --keypair "$COLD" --outfile "$WORK/memo.signed.json"
"$CLI" -C "$CONFIG" transaction verify "$WORK/memo.signed.json" --fetch-nonce
"$CLI" -C "$CONFIG" transaction simulate relay "$WORK/memo.signed.json"
"$CLI" -C "$CONFIG" transaction submit "$WORK/memo.signed.json"
```

For a custom program without a sign-only CLI, construct its instructions with the
Solana SDK. Use the PDA wherever your program requires its signing authority, set
the legacy message's blockhash to the current SPL nonce, and export this source
JSON shape (the values below are placeholders):

```json
{
  "blockhash": "NONCE_VALUE",
  "message": "BASE64_SERIALIZED_LEGACY_MESSAGE"
}
```

Then use the same `transaction create --from-sign-only` command. A source message
is not yet a wrapped transaction file. Unknown instructions display raw program,
account, and data fields during inspection; reviewing them requires knowledge of
the target program. See [the Rust client pointers](architecture.md) for integration.

## Hardware-wallet signer sources

The CLI accepts Solana signer URLs, including `usb://ledger?key=0`. Physical device
signing is not yet verified. To try it in a fresh walkthrough, replace the `COLD`
assignment in step 2 with the following **before deriving `COLD_ADDRESS` and `PDA`**:

```sh
COLD='usb://ledger?key=0'
```

Continue step 2 and create the nonce and fund the resulting PDA in step 3. The
existing `--keypair "$COLD"` commands will use that signer source. Keep the URL
quoted. An existing file prepared for a different authority cannot be signed by
substituting a different key. Signer URLs also work with `--fee-payer` and
`--submit-signer`.

## Command reference

The CLI uses standard Solana `-C`, `-u`, `--fee-payer`, commitment, and signer-source
options. Use each command's `--help` for flags. Summaries support `--output json`
and `--output json-compact`. Transaction readers accept `-` for stdin; writers
use `--outfile` or stdout.

Every command above serves a documented flow:

| Commands | Purpose | RPC |
| --- | --- | --- |
| `address` | Derive the asset-owning PDA | No |
| `nonce create`, `nonce show` | Initialize and refresh nonce state | Yes |
| `nonce advance` | Build a cancellation | Only with `--nonce`; `--from-transaction` is offline |
| `transaction create` | Wrap a source message | With `--fetch-nonce`; explicit snapshots and `--after` are offline |
| `transaction inspect`, `sign`, `merge` | Review and collect signatures; inspect also predicts the successor | No |
| `transaction verify` | Check signatures, cluster and nonce before relay | With `--fetch-nonce`; explicit snapshots are offline |
| `transaction simulate inner`, `simulate relay` | Rehearse before signing or submitting | Yes |
| `transaction submit` | Verify, relay, and confirm | Yes |

Offline creation accepts `--nonce-value` and `--genesis-hash`; verification also
requires `--nonce-authority`. These snapshots do not establish current chain state.
Submission checks the live nonce, cluster genesis hash, and all signatures again.

## Finish, restart, or troubleshoot

Stop the validator with Ctrl-C in terminal A when finished. The working files and
ledger remain under `target/`. A fresh ledger has a new genesis and nonce state;
restart the walkthrough with a fresh working directory for that ledger. Keep the
same terminal B session while following a run so its variables remain available.

- `nonce mismatch`: refresh with `nonce show` and rebuild. Replays and canceled
  files are expected to fail this way.
- Keep using the same local RPC for online commands. A different cluster fails
  genesis verification.
- The PDA needs the assets spent by its instructions; the online payer needs SOL
  for fees. Failed inner execution rolls back the nonce change.
- Existing output files are never overwritten. Use a fresh name or directory for
  another attempt.
