# Quickstart

This guide covers SOL and SPL Token transfers, offline signing, multiple cold
signatures, designated relayers, pre-signed chains, cancellation, custom
instructions, and hardware-wallet signer sources. All network examples use a local
validator with the current programs. Public deployment compatibility is unverified.

The coordinator builds a transaction file, the cold authority inspects and signs
it offline, and an online relayer pays to submit it. The PDA owns the assets; the
cold authority needs no SOL. See [how it works](architecture.md) for the three programs.

## Build and run the automated demo

Use the repository Rust toolchain and configured nightly, Solana CLI 3.1.8 with SBF
build tools, SPL Token CLI 5.5.0, Bash, Python 3, curl, and jq. Checks also require
cargo-hack. From the repository root:

```sh
make check-clients
make demo-local
# If ports 18899–18901 are occupied:
DEMO_RPC_PORT=19899 make demo-local
```

The [demo script](../scripts/demo-local.sh) creates disposable keys, a ledger, and a
mint. It checks SOL chains, replay rejection, cancellation, and token balances,
retains its files under `target/local-demo/`, and stops its validator on exit.

## Set up a manual session

In one terminal, build and start a fresh local validator. Leave it running while
following the remaining sections in a second Bash terminal from the repository root.
Stop it with Ctrl-C when finished. This uses ports 18899–18901 and does not change
your default Solana configuration.

```sh
make build-clients-cli build-sbf-nonce-program build-sbf-signer-program build-sbf-executor-program
solana-test-validator --ledger "$(mktemp -d "$PWD/target/quickstart-ledger.XXXXXX")" \
  --bind-address 127.0.0.1 --rpc-port 18899 --faucet-port 18901 \
  --bpf-program Noncediea1fH12usShuQAz28UhgAeuE5Maf32LsMUQB target/deploy/spl_nonce_program.so \
  --bpf-program EdSigVfK1DkeMrjFNDMjwfQaJPhPTtX7jW8uPv3oKEgN target/deploy/spl_ed25519_signer_program.so \
  --bpf-program ExecxgyHYsAXB4c5dZodV1zJZ9hqfsDCYkRDRATrpkFR target/deploy/spl_legacy_message_executor_program.so
```

In the second terminal, create disposable demo keys and fund the online payer:

```sh
CLI="$PWD/target/debug/spl-programmatic-signer-cli"
RPC=http://127.0.0.1:18899
DEMO=$(mktemp -d "$PWD/target/quickstart.XXXXXX")
for key in payer cold recipient; do
  solana-keygen new --silent --no-bip39-passphrase --outfile "$DEMO/$key.json"
done
PAYER="$DEMO/payer.json"
COLD="$DEMO/cold.json"
COLD_ADDRESS=$(solana-keygen pubkey "$COLD")
RECIPIENT=$(solana-keygen pubkey "$DEMO/recipient.json")
PDA=$("$CLI" address "$COLD_ADDRESS")
solana -u "$RPC" airdrop 10 "$(solana-keygen pubkey "$PAYER")"

"$CLI" -u "$RPC" --fee-payer "$PAYER" --output json-compact \
  nonce create --cold-authority "$COLD_ADDRESS" > "$DEMO/nonce.json"
NONCE_ACCOUNT=$(jq -er .nonceAccount "$DEMO/nonce.json")
NONCE_VALUE=$(jq -er .nonce "$DEMO/nonce.json")
solana -u "$RPC" -k "$PAYER" transfer "$PDA" 0.1 --allow-unfunded-recipient
```

One machine plays all three roles here. With a real cold authority, generate and
keep its key on the signing machine; give the coordinator only `COLD_ADDRESS`.
Transaction files use standard Solana Rust SDK JSON; `.source.json` files contain
the stock CLI's sign-only message dump. Neither file contains private keys.

## Prepare, inspect, sign, and submit SOL

Build a transfer from the PDA, wrap it, and simulate its inner instructions before
requesting a cold signature:

```sh
solana -u "$RPC" transfer "$RECIPIENT" 0.001 \
  --from "$PDA" --fee-payer "$PDA" --blockhash "$NONCE_VALUE" \
  --sign-only --dump-transaction-message --output json-compact \
  --allow-unfunded-recipient > "$DEMO/transfer.source.json"
"$CLI" -u "$RPC" transaction create --from-sign-only "$DEMO/transfer.source.json" \
  --nonce "$NONCE_ACCOUNT" --authority "$COLD_ADDRESS" --fetch-nonce \
  --outfile "$DEMO/transfer.json"
"$CLI" -u "$RPC" --fee-payer "$PAYER" transaction simulate inner "$DEMO/transfer.json"
```

Here `--blockhash` carries the SPL nonce value. Do not pass the stock CLI's native
`--nonce` option, which uses a different protocol. The source `--fee-payer` makes
the PDA an inner signer; the online payer pays the actual network fee later.
Inner simulation skips nonce and signature checks; relay simulation checks the
complete signed path. Neither reserves chain state.

Move `transfer.json` to the signing machine. Inspect the recipient, amount,
accounts, programs, nonce, and genesis hash, then sign. These commands need no RPC:

```sh
"$CLI" transaction inspect "$DEMO/transfer.json"
"$CLI" transaction sign "$DEMO/transfer.json" --keypair "$COLD" \
  --outfile "$DEMO/transfer.signed.json"
```

Return the signed file to the online relayer, verify the live nonce, rehearse the
relay, submit, and check the recipient's balance:

```sh
"$CLI" -u "$RPC" transaction verify "$DEMO/transfer.signed.json" --fetch-nonce
"$CLI" -u "$RPC" --fee-payer "$PAYER" transaction simulate relay "$DEMO/transfer.signed.json"
"$CLI" -u "$RPC" --fee-payer "$PAYER" transaction submit "$DEMO/transfer.signed.json"
solana -u "$RPC" balance "$RECIPIENT" --lamports
```

The recipient now holds 1,000,000 lamports. Submission reports the confirmed
signature and predicted/observed successor nonce. Sending the same file again
fails with `nonce mismatch`:

```sh
"$CLI" -u "$RPC" --fee-payer "$PAYER" transaction submit "$DEMO/transfer.signed.json"
```

## Pre-sign a chain and sign a batch

Continue in the same session. Read the current nonce and prepare the first file:

```sh
NONCE_VALUE=$("$CLI" -u "$RPC" --output json-compact nonce show "$NONCE_ACCOUNT" | jq -er .nonce)
solana -u "$RPC" transfer "$RECIPIENT" 0.001 \
  --from "$PDA" --fee-payer "$PDA" --blockhash "$NONCE_VALUE" \
  --sign-only --dump-transaction-message --output json-compact > "$DEMO/first.source.json"
"$CLI" -u "$RPC" transaction create --from-sign-only "$DEMO/first.source.json" \
  --nonce "$NONCE_ACCOUNT" --authority "$COLD_ADDRESS" --fetch-nonce --outfile "$DEMO/first.json"
```

`inspect` includes `nextNonce`, conditional on that exact message succeeding.
Use it to build the second file before submitting the first. `--after` checks
the successor against its predecessor and supplies the genesis hash without RPC:

```sh
NEXT=$("$CLI" --output json-compact transaction inspect "$DEMO/first.json" | jq -er .nextNonce)
solana -u "$RPC" transfer "$RECIPIENT" 0.001 \
  --from "$PDA" --fee-payer "$PDA" --blockhash "$NEXT" \
  --sign-only --dump-transaction-message --output json-compact > "$DEMO/second.source.json"
"$CLI" transaction create --from-sign-only "$DEMO/second.source.json" \
  --nonce "$NONCE_ACCOUNT" --authority "$COLD_ADDRESS" --after "$DEMO/first.json" \
  --outfile "$DEMO/second.json"
"$CLI" transaction inspect "$DEMO/second.json"
mkdir "$DEMO/signed"
"$CLI" transaction sign "$DEMO/first.json" "$DEMO/second.json" \
  --keypair "$COLD" --outdir "$DEMO/signed"
```

Submitting the second file first fails with `nonce mismatch`:

```sh
"$CLI" -u "$RPC" --fee-payer "$PAYER" transaction submit "$DEMO/signed/second.json"
```

Submit in order. A competing transition would invalidate the planned descendants.
Use separate nonce accounts when transactions must proceed independently.

```sh
"$CLI" -u "$RPC" --fee-payer "$PAYER" transaction submit "$DEMO/signed/first.json"
"$CLI" -u "$RPC" --fee-payer "$PAYER" transaction submit "$DEMO/signed/second.json"
```

## Cancel a pending file

Prepare and sign another transfer, but leave it unsubmitted:

```sh
NONCE_VALUE=$("$CLI" -u "$RPC" --output json-compact nonce show "$NONCE_ACCOUNT" | jq -er .nonce)
solana -u "$RPC" transfer "$RECIPIENT" 0.001 \
  --from "$PDA" --fee-payer "$PDA" --blockhash "$NONCE_VALUE" \
  --sign-only --dump-transaction-message --output json-compact > "$DEMO/pending.source.json"
"$CLI" -u "$RPC" transaction create --from-sign-only "$DEMO/pending.source.json" \
  --nonce "$NONCE_ACCOUNT" --authority "$COLD_ADDRESS" --fetch-nonce --outfile "$DEMO/pending.json"
"$CLI" transaction sign "$DEMO/pending.json" --keypair "$COLD" --outfile "$DEMO/pending.signed.json"
```

Build an empty transaction that consumes the same nonce, inspect it, sign, and submit:

```sh
"$CLI" nonce advance --from-transaction "$DEMO/pending.json" \
  --authority "$COLD_ADDRESS" --outfile "$DEMO/cancel.json"
"$CLI" transaction inspect "$DEMO/cancel.json"
"$CLI" transaction sign "$DEMO/cancel.json" --keypair "$COLD" --outfile "$DEMO/cancel.signed.json"
"$CLI" -u "$RPC" --fee-payer "$PAYER" transaction submit "$DEMO/cancel.signed.json"
```

Cancellation takes effect only when it lands. The pending transfer now fails with
`nonce mismatch`. If the pending transfer had landed first, cancellation would fail.

```sh
"$CLI" -u "$RPC" --fee-payer "$PAYER" transaction submit "$DEMO/pending.signed.json"
```

## Collect multiple cold signatures

Add a second approval authority. Every listed authority must sign; the transfer
still spends from the first authority's PDA:

```sh
solana-keygen new --silent --no-bip39-passphrase --outfile "$DEMO/cold-2.json"
COLD_ADDRESS_2=$(solana-keygen pubkey "$DEMO/cold-2.json")
NONCE_VALUE=$("$CLI" -u "$RPC" --output json-compact nonce show "$NONCE_ACCOUNT" | jq -er .nonce)
solana -u "$RPC" transfer "$RECIPIENT" 0.001 \
  --from "$PDA" --fee-payer "$PDA" --blockhash "$NONCE_VALUE" \
  --sign-only --dump-transaction-message --output json-compact > "$DEMO/multisig.source.json"
"$CLI" -u "$RPC" transaction create --from-sign-only "$DEMO/multisig.source.json" \
  --nonce "$NONCE_ACCOUNT" --authority "$COLD_ADDRESS" --authority "$COLD_ADDRESS_2" \
  --fetch-nonce --outfile "$DEMO/multisig.json"
"$CLI" transaction inspect "$DEMO/multisig.json"
"$CLI" transaction sign "$DEMO/multisig.json" --keypair "$COLD" \
  --outfile "$DEMO/multisig.first.json"
"$CLI" transaction sign "$DEMO/multisig.json" --keypair "$DEMO/cold-2.json" \
  --outfile "$DEMO/multisig.second.json"
"$CLI" transaction merge "$DEMO/multisig.first.json" "$DEMO/multisig.second.json" \
  --outfile "$DEMO/multisig.signed.json"
"$CLI" -u "$RPC" transaction verify "$DEMO/multisig.signed.json" --fetch-nonce
"$CLI" -u "$RPC" --fee-payer "$PAYER" transaction simulate relay "$DEMO/multisig.signed.json"
"$CLI" -u "$RPC" --fee-payer "$PAYER" transaction submit "$DEMO/multisig.signed.json"
```

Each authority independently inspects and signs a copy. Merge accepts only copies
of exactly the same message with valid signatures.

## Require a designated relayer

Ordinarily anyone holding a fully signed file may relay it. To restrict that,
make a separate relayer key an inner signer and name it with `--submit-signer`.
Here it is the inner fee-payer slot; the funded online payer still pays network fees:

```sh
solana-keygen new --silent --no-bip39-passphrase --outfile "$DEMO/relayer.json"
RELAYER=$(solana-keygen pubkey "$DEMO/relayer.json")
NONCE_VALUE=$("$CLI" -u "$RPC" --output json-compact nonce show "$NONCE_ACCOUNT" | jq -er .nonce)
solana -u "$RPC" transfer "$RECIPIENT" 0.001 \
  --from "$PDA" --fee-payer "$RELAYER" --blockhash "$NONCE_VALUE" \
  --sign-only --dump-transaction-message --output json-compact > "$DEMO/designated.source.json"
"$CLI" -u "$RPC" transaction create --from-sign-only "$DEMO/designated.source.json" \
  --nonce "$NONCE_ACCOUNT" --authority "$COLD_ADDRESS" --submit-signer "$RELAYER" \
  --fetch-nonce --outfile "$DEMO/designated.json"
"$CLI" transaction inspect "$DEMO/designated.json"
"$CLI" transaction sign "$DEMO/designated.json" --keypair "$COLD" \
  --outfile "$DEMO/designated.cold.json"
"$CLI" transaction sign "$DEMO/designated.cold.json" --keypair "$DEMO/relayer.json" \
  --outfile "$DEMO/designated.ready.json"
"$CLI" -u "$RPC" --fee-payer "$PAYER" transaction simulate relay "$DEMO/designated.ready.json" \
  --submit-signer "$DEMO/relayer.json"
"$CLI" -u "$RPC" --fee-payer "$PAYER" transaction submit "$DEMO/designated.ready.json" \
  --submit-signer "$DEMO/relayer.json"
```

The relayer signs both the file and the live outer transaction. A different fee
payer cannot bypass the required relayer's live signature.

## Transfer SPL tokens

Create a local six-decimal mint and two token accounts, then mint two tokens to
the PDA. These setup transactions use the online payer:

```sh
for key in mint source-token destination-token; do
  solana-keygen new --silent --no-bip39-passphrase --outfile "$DEMO/$key.json"
done
MINT=$(solana-keygen pubkey "$DEMO/mint.json")
SOURCE_TOKEN=$(solana-keygen pubkey "$DEMO/source-token.json")
DESTINATION_TOKEN=$(solana-keygen pubkey "$DEMO/destination-token.json")
spl-token -u "$RPC" create-token "$DEMO/mint.json" --decimals 6 \
  --fee-payer "$PAYER" --mint-authority "$PAYER"
spl-token -u "$RPC" create-account "$MINT" "$DEMO/source-token.json" --owner "$PDA" --fee-payer "$PAYER"
spl-token -u "$RPC" create-account "$MINT" "$DEMO/destination-token.json" --owner "$RECIPIENT" --fee-payer "$PAYER"
spl-token -u "$RPC" mint "$MINT" 2 "$SOURCE_TOKEN" --fee-payer "$PAYER" --mint-authority "$PAYER"
```

Build a checked transfer of 1.25 tokens, inspect the mint, amount and decimals,
then follow the same signing and relay flow:

```sh
NONCE_VALUE=$("$CLI" -u "$RPC" --output json-compact nonce show "$NONCE_ACCOUNT" | jq -er .nonce)
spl-token -u "$RPC" transfer "$MINT" 1.25 "$DESTINATION_TOKEN" --from "$SOURCE_TOKEN" \
  --owner "$PDA" --fee-payer "$PDA" --blockhash "$NONCE_VALUE" --mint-decimals 6 \
  --sign-only --dump-transaction-message --output json-compact > "$DEMO/token.source.json"
"$CLI" -u "$RPC" transaction create --from-sign-only "$DEMO/token.source.json" \
  --nonce "$NONCE_ACCOUNT" --authority "$COLD_ADDRESS" --fetch-nonce --outfile "$DEMO/token.json"
"$CLI" -u "$RPC" --fee-payer "$PAYER" transaction simulate inner "$DEMO/token.json"
"$CLI" transaction inspect "$DEMO/token.json"
"$CLI" transaction sign "$DEMO/token.json" --keypair "$COLD" --outfile "$DEMO/token.signed.json"
"$CLI" -u "$RPC" transaction verify "$DEMO/token.signed.json" --fetch-nonce
"$CLI" -u "$RPC" --fee-payer "$PAYER" transaction simulate relay "$DEMO/token.signed.json"
"$CLI" -u "$RPC" --fee-payer "$PAYER" transaction submit "$DEMO/token.signed.json"
spl-token -u "$RPC" balance --address "$SOURCE_TOKEN"
spl-token -u "$RPC" balance --address "$DESTINATION_TOKEN"
```

The source holds 0.75 tokens and the recipient holds 1.25. A token-owning PDA
need not hold SOL; the online payer covers account creation and relay fees.

## Build custom program instructions

For a program without a sign-only CLI, build an ordinary SDK instruction and
export the same source JSON. The small [Memo example](../clients/cli/examples/build_memo_inner.rs)
uses the PDA as a required signer and is part of the existing CLI crate:

```sh
NONCE_VALUE=$("$CLI" -u "$RPC" --output json-compact nonce show "$NONCE_ACCOUNT" | jq -er .nonce)
cargo +nightly-2026-01-22 run -q -p spl-programmatic-signer-cli --example build_memo_inner -- \
  "$PDA" "$NONCE_VALUE" 'custom instruction demo' > "$DEMO/memo.source.json"
"$CLI" -u "$RPC" transaction create --from-sign-only "$DEMO/memo.source.json" \
  --nonce "$NONCE_ACCOUNT" --authority "$COLD_ADDRESS" --fetch-nonce --outfile "$DEMO/memo.json"
"$CLI" -u "$RPC" --fee-payer "$PAYER" transaction simulate inner "$DEMO/memo.json"
"$CLI" transaction inspect "$DEMO/memo.json"
"$CLI" transaction sign "$DEMO/memo.json" --keypair "$COLD" --outfile "$DEMO/memo.signed.json"
"$CLI" -u "$RPC" transaction verify "$DEMO/memo.signed.json" --fetch-nonce
"$CLI" -u "$RPC" --fee-payer "$PAYER" transaction simulate relay "$DEMO/memo.signed.json"
"$CLI" -u "$RPC" --fee-payer "$PAYER" transaction submit "$DEMO/memo.signed.json"
```

Simulation logs should include `Signed by` followed by the PDA and the memo text.
For your program, replace the instruction's program ID, accounts, and data.
Unknown instructions display raw data during inspection, so the signer needs a
trusted way to understand those bytes.

## Use a hardware wallet

For a separate run, replace the cold-key setup with a Solana signer URL and derive
its public address before creating the nonce account and funding the new PDA:

```sh
COLD='usb://ledger?key=0'
COLD_ADDRESS=$(solana address -k "$COLD")
PDA=$("$CLI" address "$COLD_ADDRESS")
```

Then repeat the prepare/inspect/sign/submit flow in a fresh output directory using
that authority. The existing `--keypair "$COLD"` command prompts the device to sign.
Signer URLs also work with `--fee-payer` and `--submit-signer`. Quote URLs in the
shell. Physical hardware signing has not been verified in the local demo.

## Command reference and troubleshooting

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

- `nonce mismatch`: refresh with `nonce show` and rebuild. Replays and canceled
  files are expected to fail this way.
- Keep using the same local RPC for online commands. A different cluster fails
  genesis verification.
- The PDA needs the assets spent by its instructions; the online payer needs SOL
  for fees. Failed inner execution rolls back the nonce change.
- Existing output files are never overwritten. Use a fresh name or directory for
  another attempt. See [client usage](clients.md) for JSON IO and snapshot options.
