# Clients

## CLI

Build with `make build-clients-cli`. The package and executable are both named
`spl-programmatic-signer-cli`. Use `--help` on each subcommand for all flags.
Global configuration follows the Solana CLI: `-C`, `-u`, `--fee-payer`,
`--commitment`, and `--skip-preflight`. Display commands support `--output json`
and `--output json-compact`. JSON output structs are exposed by the CLI library's
`output` module and reused by integration tests.

| Command | Purpose | RPC |
| --- | --- | --- |
| `address AUTHORITY` | Derive the programmatic signer PDA | No |
| `nonce create` | Create and verify a current nonce account; keypair optional | Yes |
| `nonce show ACCOUNT` | Decode account ownership and state | Yes |
| `nonce advance` | Build an unsigned cancellation | Only with `--nonce` |
| `transaction create` | Import stock CLI sign-only JSON | Only with `--fetch-nonce` |
| `transaction inspect FILE` | Review signed bytes and decoded instructions | No |
| `transaction sign FILE...` | Add signatures, preserving existing valid ones | No |
| `transaction merge FILE FILE...` | Merge signatures for identical messages | No |
| `transaction next-nonce FILE` | Predict the conditional successor | No |
| `transaction verify FILE` | Check signatures, genesis, and nonce snapshot | Optional |
| `transaction simulate inner FILE` | Simulate business instructions | Yes |
| `transaction simulate relay FILE` | Simulate full signed execution | Yes |
| `transaction submit FILE` | Verify, assemble, send, and confirm relay | Yes, except `--no-send` |

### Import and verification

`transaction create` requires `--from-sign-only`, `--nonce`, at least one
`--authority`, and exactly one nonce source:

- `--fetch-nonce` queries account state and cluster genesis.
- `--nonce-value HASH --genesis-hash HASH` uses an offline snapshot. The nonce
  authority defaults to the first cold authority's PDA; override with
  `--nonce-authority ADDRESS` when necessary.
- `--after FILE` takes the predecessor's nonce account and genesis context and
  predicts its successor. The imported inner message must already use that hash.

`--submit-signer ADDRESS` can repeat for designated live signers. These must be
represented in the inner message. Source CLI signatures do not sign the wrapper;
only the dumped message is imported. Both Solana and SPL Token JSON are supported.
`badSig` entries, missing messages, mismatched hashes, and unsupported message
versions are rejected.

`transaction verify --fetch-nonce` verifies the current cluster and account. Offline
verification needs `--nonce-value`, `--nonce-authority`, and `--genesis-hash`.
Use `--allow-partial` while collecting signatures; full verification requires every
slot. Offline snapshots can become stale and do not prove current on-chain state.

### Files and signers

A `.psigner` file contains base64-encoded, serialized Solana `VersionedTransaction`
bytes, including its partial signature slots. Its inner nonce and wrapper genesis
hash are lifetime fields for this protocol, not a fresh network blockhash. Do not
send this wrapper directly with ordinary `sendTransaction`; build the relay first.
Readers accept `-` for stdin and bound input to 1 MiB. Writers use `--outfile`,
stdout when omitted, or `--outfile -`. Existing paths are never overwritten.
`--output` controls summaries; transaction-file output remains base64.

```sh
spl-programmatic-signer-cli transaction sign unsigned.psigner \
  --keypair cold-a.json --outfile partial-a.psigner
spl-programmatic-signer-cli transaction sign unsigned.psigner \
  --keypair cold-b.json --outfile partial-b.psigner
spl-programmatic-signer-cli transaction merge partial-a.psigner partial-b.psigner \
  --outfile complete.psigner
mkdir signed
spl-programmatic-signer-cli transaction sign first.psigner second.psigner \
  --keypair cold-a.json --outdir signed
```

Multiple inputs require an existing `--outdir`. Duplicate output names and existing
files are rejected. Signer paths use the standard Solana loader, including keypair
files, `ASK`, `prompt://`, and `usb://ledger` where supported. Hardware signing has
not been tested on a physical device for this branch; decoded CLI inspection does
not imply hardware clear-signing support.

For a designated relayer, pass `--submit-signer SOURCE` to `submit` or
`simulate relay`. Every required live signer must sign the wrapper file first as
well. To construct a relay offline, use `submit --no-send --blockhash HASH
--outfile relay.base64` with the online fee payer and required live signer sources.
This skips live RPC verification, so verify against current state before sending.

Inspection decodes system transfers, classic SPL Token transfers and checked
transfers, and UTF-8 Memo instructions. Unknown instructions retain program IDs,
account addresses, and base64 data. An unchecked token transfer does not encode its
mint or decimals, so inspection reports raw units without inferring either.

## Rust

`spl-programmatic-signer-client` composes the current per-program clients and has no
RPC or wallet dependencies. `TransactionPlan::new` accepts arbitrary instructions,
cold authorities, designated submit signers, and a nonce account. Convenience
constructors cover SOL transfers and cancellations.

The flow is `build_transaction` (or a `transaction_from_*` import), `inspect`,
`sign_transaction`, optional `merge_transactions`, `verify`, and
`submit_transaction`. Prefer the `*_checked` constructors when a nonce snapshot is
available. `nonce::next_nonce` computes a conditional successor for chain planning.
`submit_transaction` builds and signs the network relay; callers own RPC fetching,
confirmation, and error handling.

Runnable, RPC-free examples use freshly generated demo authorities and synthetic
nonce/genesis hashes. They do not submit anything:

```sh
make example-clients-rust ARGS='--example offline_transfer'
make example-clients-rust ARGS='--example custom_instruction'
```

See [`offline_transfer.rs`](../clients/rust/examples/offline_transfer.rs) and
[`custom_instruction.rs`](../clients/rust/examples/custom_instruction.rs). For real
submission, replace the synthetic snapshot with authenticated current account data,
check account ownership, verify cluster genesis, and fetch a fresh relay blockhash.

The per-program crates retain their existing no-std interfaces and instruction
builders. The higher-level workflow crate intentionally uses `std`. Generated
JavaScript clients continue to use `make generate-clients`; they have not been
replaced by the Rust workflow layer.
