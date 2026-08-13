# Migration

The migration path from the system durable-nonce offline signing workflow is
operational. Existing system durable-nonce accounts have nothing to convert on-chain.
Operators open new SPL Nonce accounts, move their signing runbooks to transaction
files, then reclaim rent from retired system nonce accounts when no old sign-only
payloads remain.

## Concept mapping

The core offline signing workflow translates directly to the programmatic system, but
the replay boundary moves inward.

- System nonce account <-> nonce account.
- Nonce authority <-> nonce account authority.
- `--blockhash <NONCE>` <-> the inner message's lifetime specifier (the field
  legacy messages call `recent_blockhash`) carrying the nonce account nonce.
- Cluster choice <-> the cold-signed transaction's lifetime specifier carrying
  the cluster genesis hash.
- `--sign-only` signer pairs <-> the transaction file.
- Whole durable-nonce transaction <-> the inner message replayed through the
  executor.
- Transaction fee payer <-> relayer fee payer for the hot relay transaction.

A system nonce account and an SPL Nonce account are different account types. There is no
state migration between them. The system account stores System Program nonce state.
The nonce account stores the SPL Nonce program's 64-byte `{ nonce, authority }` state. Migration
means creating nonce accounts and changing procedures.

The authority model broadens. A system durable nonce is advanced by a keypair
authority. A nonce account authority may be a keypair or any PDA that can arrive with runtime
signer privilege. The SPL Nonce program itself is independent of the signer program.
It only requires the stored authority account to be a signer at `Advance` time.

The lifetime specifier also changes meaning. In the system workflow, the durable nonce is
the transaction's recent blockhash and the first instruction advances the nonce. In
the programmatic workflow, the inner message's lifetime specifier carries the nonce account nonce. The
executor checks it and advances the nonce immediately before replay. If replay fails,
transaction rollback restores the old nonce.
The cold-signed transaction's lifetime specifier is not used for transaction loading
because the runtime never evaluates that transaction on its own. Tooling uses it to
carry the cluster genesis hash, covered by the cold signatures and checked by
`verify`, not by the programs.

Both systems still give one serial lane per nonce account. If two outstanding
transaction files use the same nonce account, exactly one can land. If two transactions must both land, open
two nonce accounts under the same authority.

## Command mapping

The `psigner` CLI now implements the MVP transaction-file flow in `clients/cli`. It
mirrors the existing `solana` durable-nonce workflow step by step: build a
transaction with the existing Solana or SPL Token CLI, dump a sign-only message, wrap
it in a transaction file, inspect and sign offline, then return the file to an online
relayer. The commands look familiar, but the fee payer, the handoff, and the advance
timing differ.

| System CLI                                                                                                                                                                                        | Programmatic CLI                                                                                                                                                   | Semantic difference                                                                                                                                                                                                                                                 |
|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `solana create-nonce-account`                                                                                                                                                                     | `psigner nonce create`                                                                                                                                             | Creates a new nonce account rather than converting an old account. A funded online wallet can create and initialize the nonce account for a direct nonce authority or for a derived ProgrammaticSigner authority. The authority does not sign setup.                |
| `solana nonce-account`                                                                                                                                                                            | `psigner nonce show`                                                                                                                                               | Reads current nonce state. The programmatic state is the SPL Nonce account hash plus authority, not System Program nonce state.                                                                                                                                     |
| `solana new-nonce`                                                                                                                                                                                | `psigner nonce advance` + `psigner transaction sign` + `psigner transaction submit` for PDA authorities, or the Rust helper direct advance for keypair authorities | PDA-authority cancellation follows the same transaction-file path: build the nonce-consuming file, then the cold signer signs and the relayer submits. Keypair-authority direct advance is exposed in Rust helpers and needs a dedicated CLI path later.            |
| `solana ... --blockhash <NONCE> --sign-only --dump-transaction-message --output json-compact` or `spl-token ... --blockhash <NONCE> --sign-only --dump-transaction-message --output json-compact` | `psigner transaction create --from-sign-only inner.json --nonce <NONCE_ACCOUNT> --authority <AUTHORITY> [--fetch-nonce                                             | --nonce-value <HASH> --genesis-hash <HASH>] --outfile tx.psigner`                                                                                                                                                                                                   | Existing Solana/SPL CLIs still construct the inner transaction message. `psigner` wraps that dumped message into a transaction file and binds the cold-signed transaction to the cluster genesis hash. Add `--fetch-nonce` online, or `--nonce-value <HASH> [--nonce-authority <ADDRESS>] --genesis-hash <HASH>` offline, to verify the message against a nonce snapshot while creating it. |
| Paste signer pairs                                                                                                                                                                                | `psigner transaction sign tx.psigner --keypair cold.json --outfile tx.signed.psigner`                                                                              | System flows move detached signer-pair strings back to the online machine. Programmatic flows add signatures into the standard transaction slots inside the file. The signing device must decode and inspect the file before signing.                               |
| `solana ... --blockhash <NONCE> --signer <PAIR>`                                                                                                                                                  | `psigner transaction submit tx.signed.psigner --fee-payer hot.json`                                                                                                | The system command submits the same whole transaction using collected signer pairs and that transaction's fee payer. The programmatic command builds the hot relay transaction. The relayer pays the outer fee and the cold authorities hold no SOL just to submit. |
| `solana withdraw-from-nonce-account`                                                                                                                                                              | No nonce account conversion command                                                                                                                                | After old system nonce accounts are retired, reclaim their rent with the normal system CLI. SPL Nonce accounts are separate accounts and are not created from those retired accounts.                                                                               |

An operator runbook usually changes in this order.

1. Inventory current durable-nonce workflows.
   Record each system nonce account, nonce authority, fee payer, destination
   accounts, and outstanding sign-only payloads.

2. Decide the nonce account authority shape.
   Use a keypair authority when the same cold key should directly authorize nonce account
   cancellation through the Rust helper's direct advance instruction or a future CLI path.
   Use a PDA authority for the current `psigner nonce advance` cancellation flow, where the
   signer program promotes a `ProgrammaticSigner`.

3. Create replacement nonce accounts.
   Run `psigner nonce create --programmatic-authority <AUTHORITY>
   --nonce-keypair nonce.json --fee-payer payer.json` once per serial nonce account
   you need. Use `--nonce-authority <ADDRESS>` instead when the nonce account should
   be advanced directly by a keypair or another non-programmatic authority.
   Create more than one nonce account for workflows that need parallel submissions. Use
   `psigner nonce show <NONCE_ACCOUNT>` to record each nonce account's nonce and authority in the
   operations inventory.

4. Update the transaction-building runbook.
   Keep using the existing Solana or SPL Token command that knows how to build the
   inner transaction, but run it with `--blockhash <NONCE> --sign-only
   --dump-transaction-message --output json-compact > inner.json`. Then run
   `psigner transaction create --from-sign-only inner.json --nonce <NONCE_ACCOUNT>
   --authority <AUTHORITY> --fetch-nonce --outfile tx.psigner`. The coordinator reads the
   nonce account, imports the inner message with the nonce account nonce in its
   lifetime specifier, wraps it in an executor instruction, wraps that in the
   cold-signed transaction, sets the cold-signed transaction's lifetime specifier
   to the cluster genesis hash, and writes the transaction file.
   The source command must already include the signer keys that the executor will
   promote. For SPL Token multisig, use the SPL Token multisig account as `--owner`,
   pass each required ProgrammaticSigner PDA with repeated `--multisig-signer`, and
   pass the backing cold authorities to `psigner transaction create` with repeated
   `--authority`. If a designated submit signer gates submission, also put that key
   in the source message, commonly as `--fee-payer <SUBMIT_SIGNER_PUBKEY>`, before
   passing it as `--submit-signer`.

5. Update the signing runbook.
   Move the transaction file to the signing device. Run `psigner transaction inspect
   tx.psigner` on that device before signing. The inspection must show the
   program ids, nonce account, expected nonce, authority signer status, and decoded
   transaction. Only then run `psigner transaction sign
   tx.psigner --keypair cold.json --outfile tx.signed.psigner`. Inspection and signing must
   work with no RPC.

6. Update the submit runbook.
   Move the signed transaction file back online. Run `psigner transaction verify
   tx.signed.psigner --fetch-nonce` against a fresh nonce account snapshot before paying fees. `transaction submit`
   performs the same live nonce verification by default unless `--skip-verify` is passed. Then run
   `psigner transaction submit tx.signed.psigner --fee-payer hot.json`. The
   relayer pays the hot relay transaction fee and reports the signature plus the
   advanced nonce after confirmation. Program errors are currently surfaced through
   RPC error messages. Typed program-error decoding is follow-on relayer UX work.

7. Retire old system nonce accounts.
   Wait until no sign-only payloads or runbooks still depend on each system nonce
   account. Then use `solana withdraw-from-nonce-account` to reclaim rent from the
   retired account.

The old and new workflows can run side by side during rollout. A system nonce account
continues to protect transactions built for the old flow. A nonce account protects
transaction files built for the new flow. They do not share replay state.

A cutover is complete when these checks are true.

- Every active runbook names a nonce account instead of a system nonce account.
- Every signer has an inspection path that works without RPC.
- Every relayer verifies a fresh nonce account snapshot before submit.
- No outstanding signer-pair strings depend on the retired system nonce accounts.

## Migration support

The migration preserves the existing operational security practices.

Operators still build a transaction online, inspect it offline, sign it, and return
the signature to the online relayer. The important procedural change is that a single
transaction file replaces pasted signer-pair strings. The file carries standard
Solana transaction bytes. The current CLI stores those bytes as base64 text. The
cluster genesis hash is bound into the cold-signed transaction's signed lifetime
specifier, not stored as unsigned envelope metadata.

Client support must provide three concrete things.

1. Transaction file decode and inspection on the signing device.
   The signing surface must decode the file without RPC and display the actual
   transaction. For a transfer, that means source, destination, amount, nonce account,
   current nonce, authority, signer status, genesis hash, and program ids.
   Hardware wallets may show the wrapped payload as opaque bytes until a clear-signing
   plugin exists, so tooling-level inspection is the primary security surface.

2. Machine preflight against a nonce account snapshot.
   Before signing, clients should verify that the inner message's lifetime specifier equals the
   stored nonce account nonce from the snapshot, that the nonce account authority is
   one of the inner message's required signers, that the inner message uses no address
   table lookups, that account keys are not duplicated, that the cold-signed
   transaction's signed genesis hash matches the target cluster, and that the
   signer, executor, and nonce program ids match the configured
   deployment. Before submit, the relayer should repeat this check against a fresh
   snapshot before paying the outer fee.

3. Explicit no-state-migration behavior.
   Clients should not offer a command that claims to convert a system nonce account
   into a nonce account. The on-chain state does not migrate. The correct flow is create new
   nonce accounts, move runbooks, then withdraw rent from retired system nonce accounts.

Fee handling should be visible in every client surface. Creating a nonce account
costs the online funder the account rent and transaction fee. Transaction creation
and inspection are local file operations. Signing is offline and pays no fee.
Submitting pays the hot relay transaction fee from the relayer. The cold authority or
PDA does not need SOL unless the inner message itself intentionally spends SOL.

Revocation also changes by authority type. A keypair nonce account authority can
consume the nonce account with a direct `Advance` instruction. The Rust helper
exposes that instruction builder, while the first CLI binary focuses on the
programmatic PDA authority path. A PDA nonce account authority cannot sign a
transaction by itself. It cancels with `psigner nonce advance --from-transaction ...
--authority <AUTHORITY>`, which emits an empty-inner-message transaction file on the
same nonce account and consumes the nonce through the executor's pre-replay
`Advance`.

Pre-1.0 test deployments need conservative operations. Program bytes, program ids,
and wire formats may change. New program ids and wire format changes invalidate
in-flight transaction files, while in-place upgrades do not. Around a test
deployment upgrade, land the files you intend to keep, cancel the ones you do not,
and rebuild afterward. The final deployment will be immutable.
