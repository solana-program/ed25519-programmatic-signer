# Local quickstart

## Prerequisites

Use the repository Rust toolchain, the nightly reported by `make rust-toolchain-nightly`,
Solana CLI 3.1.8 (including `solana-test-validator` and SBF build tools), SPL Token CLI
5.5.0, Bash, Python 3, curl, and jq. `make check-clients` additionally requires
`cargo-hack`. Full repository CI checks also use pnpm, Codama, cargo-audit, and
cargo-spellcheck. Build tools may download dependencies; all demo transactions use
an isolated validator bound to `127.0.0.1`.

```sh
make check-clients
make demo-local
# If the default demo ports are occupied:
DEMO_RPC_PORT=19899 make demo-local
```

The demo uses a fresh directory and ledger on each run, funds its payer at genesis,
and loads the three current programs directly into that local genesis. It never
changes the default Solana configuration. Ports 18899–18901 must be free by default.
Its final output identifies the directory containing unsigned and signed files,
source CLI JSON, submission results, configuration, and validator logs. Its generated
keys are disposable demo keys. The validator stops when the script exits.

The script verifies two ordered SOL transfers, rejection of the second transaction
before its predecessor, rejection of a replay, cancellation of a pending transfer,
and an SPL Token transfer of 1.25 tokens from a fresh six-decimal mint. No external
mint or faucet is required. See [`scripts/demo-local.sh`](../scripts/demo-local.sh)
for the complete executable sequence.

## Prepare, inspect, sign, relay

For manual use against a local validator running the current programs, define your
own `RPC`, funded online `PAYER` keypair path, offline `COLD` keypair path, and
`RECIPIENT` address. The commands below use Bash. Keep the real cold key on the
signing machine; only its public address belongs on the online machine.

```sh
CLI="$PWD/target/debug/spl-programmatic-signer-cli"
COLD_ADDRESS=$(solana-keygen pubkey "$COLD")
PDA=$("$CLI" address "$COLD_ADDRESS")
"$CLI" -u "$RPC" --fee-payer "$PAYER" --output json-compact \
  nonce create --cold-authority "$COLD_ADDRESS" > nonce.json
NONCE_ACCOUNT=$(jq -er .nonceAccount nonce.json)
NONCE_VALUE=$(jq -er .nonce nonce.json)

# Fund the PDA with the SOL that its inner transfer will spend.
solana -u "$RPC" -k "$PAYER" transfer "$PDA" 0.1 --allow-unfunded-recipient
solana -u "$RPC" transfer "$RECIPIENT" 0.001 \
  --from "$PDA" --fee-payer "$PDA" --blockhash "$NONCE_VALUE" \
  --sign-only --dump-transaction-message --output json-compact \
  --allow-unfunded-recipient > transfer.json
"$CLI" -u "$RPC" transaction create --from-sign-only transfer.json \
  --nonce "$NONCE_ACCOUNT" --authority "$COLD_ADDRESS" --fetch-nonce \
  --outfile transfer.psigner
```

The stock CLI's `--blockhash` carries the SPL nonce value in this inner message.
Do not pass its native `--nonce` option: that creates a different nonce protocol.
The source `--fee-payer` identifies an inner signer; the hot relay pays the actual
network fee later.

Move `transfer.psigner` to the offline signing machine. Inspect the actual message,
including the recipient, amount, nonce account, expected nonce, program IDs, and
cluster genesis hash, before signing. These commands do not query RPC:

```sh
"$CLI" transaction inspect transfer.psigner
"$CLI" transaction sign transfer.psigner --keypair "$COLD" \
  --outfile transfer.signed.psigner
```

Return the signed file to the online machine:

```sh
"$CLI" -u "$RPC" transaction verify transfer.signed.psigner --fetch-nonce
"$CLI" -u "$RPC" --fee-payer "$PAYER" transaction simulate relay transfer.signed.psigner
"$CLI" -u "$RPC" --fee-payer "$PAYER" transaction submit transfer.signed.psigner
```

Submission reports the confirmed signature and both the predicted and observed
successor nonce. A later transaction may already have advanced the observed state.
A replay is rejected because the old nonce no longer matches.

## Precompute a chain or cancel a pending file

`transaction next-nonce transfer.psigner` predicts the nonce conditional on that
exact inner message succeeding. Build the next stock CLI message with that hash,
then import it with `transaction create ... --after transfer.psigner`. Both files
can be signed offline before either is submitted, but they must land in order.
A competing transaction or cancellation invalidates the planned descendants.

```sh
"$CLI" nonce advance --from-transaction transfer.psigner \
  --authority "$COLD_ADDRESS" --outfile cancel.psigner
"$CLI" transaction sign cancel.psigner --keypair "$COLD" --outfile cancel.signed.psigner
"$CLI" -u "$RPC" --fee-payer "$PAYER" transaction submit cancel.signed.psigner
```

Building or signing a cancellation does not invalidate anything. Its submission
must succeed before the competing transaction. The CLI cancellation flow supports
nonce accounts controlled by the selected cold authority's PDA.
