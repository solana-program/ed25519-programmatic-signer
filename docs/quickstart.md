# Quickstart

Use the repository Rust toolchain, its configured nightly, Solana CLI 3.1.8 with SBF
build tools, SPL Token CLI 5.5.0, Bash, Python 3, curl, and jq. The checks also require
cargo-hack. Public deployment compatibility is unverified; the demo runs locally.

```sh
make check-clients
make demo-local
# Choose another port if 18899–18901 are occupied:
DEMO_RPC_PORT=19899 make demo-local
```

The [demo script](../scripts/demo-local.sh) creates its own keys, ledger, funded payer,
configuration, and token mint. It imports real `solana` and `spl-token` sign-only JSON,
submits two pre-signed transfers in order, rejects replays, cancels a pending file,
and checks token balances. It changes no default configuration. The final output
shows the directory containing transaction files and logs.

## Manual flow

With the current programs running locally, set `RPC` to their RPC URL, `PAYER` to a
funded online keypair path, `COLD_ADDRESS` to the cold authority's public address,
and `RECIPIENT` to the destination address.

```sh
CLI="$PWD/target/debug/spl-programmatic-signer-cli"
PDA=$("$CLI" address "$COLD_ADDRESS")
"$CLI" -u "$RPC" --fee-payer "$PAYER" --output json-compact \
  nonce create --cold-authority "$COLD_ADDRESS" > nonce.json
NONCE_ACCOUNT=$(jq -er .nonceAccount nonce.json)
NONCE_VALUE=$(jq -er .nonce nonce.json)
solana -u "$RPC" -k "$PAYER" transfer "$PDA" 0.1 --allow-unfunded-recipient

solana -u "$RPC" transfer "$RECIPIENT" 0.001 \
  --from "$PDA" --fee-payer "$PDA" --blockhash "$NONCE_VALUE" \
  --sign-only --dump-transaction-message --output json-compact \
  --allow-unfunded-recipient > transfer.json
"$CLI" -u "$RPC" transaction create --from-sign-only transfer.json \
  --nonce "$NONCE_ACCOUNT" --authority "$COLD_ADDRESS" --fetch-nonce \
  --outfile transfer.psigner
```

Here `--blockhash` carries the SPL nonce value. Do not pass the stock CLI's native
`--nonce` option, which uses a different protocol. The PDA supplies the inner transfer's
funds; the online payer pays the network fee when the wrapper is relayed.

Move the file to the offline machine, inspect its recipient, amount, accounts,
programs, nonce, and genesis hash, then sign it with `COLD`, the cold keypair path:

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

Submission reports its confirmed signature and predicted/observed successor nonce.
A replay fails once the nonce advances. See [client usage](clients.md) for partial
signatures, chains, and cancellation.
