# Client usage

Build with `make build-clients-cli`; the executable is
`target/debug/spl-programmatic-signer-cli`. It uses standard Solana `-C`, `-u`,
`--fee-payer`, commitment, and signer-source options. Use each command's `--help`
for flags. Summaries support `--output json` and `--output json-compact`.

`transaction inspect`, `sign`, `merge`, and `next-nonce` work offline. Creation and
verification accept either `--fetch-nonce` or explicit nonce/genesis snapshots.
Offline snapshots do not establish current chain state. Simulation and submission
query RPC; submission checks the live nonce, genesis hash, and all signatures.

A `.psigner` file is a base64-encoded Solana transaction containing the wrapper and
partial signature slots. Relay it with this CLI rather than sending the wrapper
as a network transaction. Readers accept `-` for stdin; writers use `--outfile` or
stdout. Existing files are never overwritten.

## Multiple signatures

Signers may produce separate copies of the same message, then merge them:

```sh
spl-programmatic-signer-cli transaction sign unsigned.psigner --keypair cold-a.json --outfile a.psigner
spl-programmatic-signer-cli transaction sign unsigned.psigner --keypair cold-b.json --outfile b.psigner
spl-programmatic-signer-cli transaction merge a.psigner b.psigner --outfile signed.psigner
```

For a batch, pass several input files and an existing `--outdir`. A designated
submit signer must be an inner signer, sign the wrapper, and sign the live relay
using `submit --submit-signer SOURCE`. Choosing a fee payer alone does not restrict
who may relay a signed file. Standard Solana signer sources are supported; physical
hardware-wallet signing remains unverified.

## Chains and cancellation

`transaction next-nonce FILE` predicts the successor conditional on that exact inner
message succeeding. Build the next source message with that hash, then import it
using `transaction create ... --after FILE`. Files can be signed together offline
but must land in order.

`nonce advance --from-transaction FILE --authority COLD_ADDRESS --outfile cancel.psigner`
builds an empty transaction for the cold authority's PDA. Sign and submit it before
the competing transaction to cancel that nonce. Creating the cancellation file alone
does not change chain state. A competing transition also invalidates planned descendants.

## Inspection and simulation

Inspection decodes SOL transfers, classic SPL Token transfers, checked transfers,
and Memo text. Unknown instructions retain their program, accounts, and raw data.
Unchecked token transfers show raw units because their mint and decimals are absent.
`simulate inner` uses the online fee payer and skips nonce/signature checks;
`simulate relay` checks the entire signed path. Neither reserves chain state.

## Rust

Compose ordinary SDK messages with the existing per-program clients:
`executor/client` builds `execute`; `signer/client` builds `wrapped_message` and
`submit`; `nonce/client` creates and decodes nonce accounts. Set the inner message's
blockhash to the nonce and the wrapper's blockhash to the cluster genesis hash before
signing. Custom instructions use the same path. There is no separate workflow SDK.
