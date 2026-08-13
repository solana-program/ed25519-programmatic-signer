# Client and tooling plan

How people actually use the signer, executor, and nonce programs: Rust helpers, a CLI,
and eventually a web stack that let anyone act as the cold signer, the coordinator, or
the hot relayer without touching wire formats by hand.

## Research anchors

Four facts shape every decision below.

**1. The cold-signed transaction is a standard Solana transaction.** The transaction a
cold wallet signs is a legacy `VersionedTransaction`, byte-for-byte the format the whole
ecosystem already signs and moves around. Ledger signs it via `solana-remote-wallet`.
Browser wallets sign it via the standard `signTransaction` request. `@solana/kit`
already partial-signs, serializes to base64 wire format, and deserializes it. We do not
need to invent a transport format for offline signing. We need to wrap an existing
one.

**2. Solana already has an offline-signing idiom, and our users already know it.** The
CLI's durable-nonce workflow (`--sign-only`, `--blockhash <nonce>`, signer pairs on
stdout, `--signer` on the online side) is the established muscle memory for air-gapped
signing. This system is philosophically "durable nonces generalized to program-level
messages", so the CLI should feel like those commands, not like a new paradigm.

**3. wincode is bincode-compatible.** The instruction data our interfaces define is a
one-byte tag followed by standard-wire-format message/transaction bytes. A TypeScript
client therefore needs no wincode port and no WASM bridge for the core path: kit's
existing transaction codecs plus a tag byte reproduce our instruction data. This must be
covered by fixture tests against real Solana/SPL Token CLI output and Rust helper
round trips, not assumed.

**4. Hardware wallets display our payload as an opaque blob.** A Ledger shows the cold-signed transaction
as "unknown program, N accounts, data bytes". The security of the flow therefore
rests on tooling-level verification: every surface must be able to decode a transaction
file back to a human-readable transaction ("transfer 5 SOL from your PDA to X, consuming nonce N"),
and the recommended practice verifies on a second device. A Ledger clear-signing plugin
is a future workstream, not a launch dependency.

## The three roles

```text
  COORDINATOR (online)            COLD SIGNER (air-gapped)         RELAYER (online, pays)
  ────────────────────            ────────────────────────         ──────────────────────
  reads nonce, builds the         verifies the decoded             verifies signatures
  inner message, wraps it   ──▶   transaction file, signs it ──▶   assembles hot relay tx
  into the cold-signed tx,        (Ledger/keypair/browser          submits, confirms,
  emits the transaction file      wallet), re-emits the file       reports errors
```

One person can play all three roles (hot-wallet convenience flow), or three different
parties on three machines (the target cold-custody flow). Multi-authority flows
fan the middle step out to several signers who each add a signature to the same
transaction file. Signatures are position-independent so collection order does not matter.

## The transaction file

Three nested objects and one file carry every flow. The canonical terms:

- **hot relay transaction** — the live transaction the relayer builds at submit
  time. It pays the fee and calls the signer program. It is never a file.
- **cold-signed transaction** — the standard `VersionedTransaction` the cold
  authorities sign. It may be unsigned, partially signed, or fully signed as it
  moves through the workflow.
- **inner message** — the `VersionedMessage` the executor replays. It carries
  the user's actual instructions and is never signed.
- **transaction file** — the cold-signed transaction as base64 text, the same
  bytes `wincode` deserializes as a `VersionedTransaction`. The `.psigner`
  extension is only a convention for those bytes. Avoid `.json` extensions,
  since the file is bare base64, not JSON.

The transaction file is the handoff artifact that moves between the three
roles, and it is the single interop point.

- The file deliberately has no JSON envelope for MVP. That keeps it compatible
  with ecosystem transaction codecs and avoids unsigned advisory metadata that
  can contradict the signed bytes.
- The cold-signed transaction's lifetime specifier (the field legacy messages
  call `recent_blockhash`) carries the cluster genesis hash. This is a client-side convention, not a program check: the
  runtime never evaluates the cold-signed transaction on its own (it travels
  inside the hot relay transaction), so the field is free to carry a cluster
  label covered by the cold signatures. `inspect` displays it, and `verify`
  checks it against an explicit `--genesis-hash` or a genesis hash fetched
  from RPC. The programs ignore this field.
- The inner message's lifetime specifier carries the nonce account nonce. The
  source Solana/SPL Token CLI puts it there with the usual `--blockhash <NONCE>`
  flag (the flag name predates the v1 naming), and the executor verifies it
  against the nonce account before replay.
- The transaction bytes are canonical. The file carries no redundant copies of
  the inner message, program ids, signer status, or cluster labels: everything
  for display is derived by decoding, and nothing on the signing side ever
  re-serializes. Redundant copies would create a consistency surface that then
  needs its own verification.
- Decoding for display derives everything else: the executor instruction, the
  inner message, the nonce account and expected nonce, signer status, and
  per-instruction summaries.
- Transports: file (`.psigner` by convention), stdin/stdout
  pipes, QR code chunks for air-gap (fits comfortably: one-instruction inner
  messages ≈ 400-700 bytes).
- Signatures accumulate in the standard signature slots. "Fully signed" is
  checkable offline (every required slot non-default).

## Nonce lifecycle

A nonce account has exactly three moves: open it once, consume it on every landed transaction file,
and advance it manually to revoke anything signed but not yet landed.

**Setup, once per nonce account.** `nonce create` sends one transaction: a system
`create_account` funding rent for the 64-byte state with the SPL Nonce program as
owner, then `Initialize`, which binds the authority and derives the first nonce from
the account address and a recent slot hash. The authority is a plain readonly account
in `Initialize` and never signs setup, so any funded wallet can open nonce accounts for any
authority and provisioning never touches cold keys. `nonce show` reads back the nonce account
to hand to a coordinator. Transaction files that must not compete get their own nonce accounts (see
Nonce concurrency).

**Consumption, every submit.** Every landed submission consumes its nonce: after
validation, the executor CPIs `Advance` before replaying the inner instructions. This
blocks recursive reuse of the nonce. Transaction rollback keeps the advance atomic
with the replay, so a failed inner instruction restores the old nonce. A successful
submission invalidates the submitted transaction file, every copy of it, and every
other outstanding file on the same nonce account. `transaction submit` reports the
advanced nonce on confirmation. When two transaction files race one nonce account,
exactly one lands and the other dies with `NonceMismatch`.

**Cancellation, revoking a signed transaction file.** Signatures cannot be recalled, so
revocation means consuming the nonce account's nonce before the unwanted file lands. The
flow depends on who the nonce authority is.

- Keypair authority: one direct `Advance` transaction. The authority signs it and
  presents the stored nonce. The Rust helper exposes the direct advance instruction;
  the first CLI binary focuses on the ProgrammaticSigner/PDA cancellation path below.
- PDA authority: the PDA cannot sign a transaction on its own, and wrapping a second
  `Advance` inside the inner message self-destructs: the executor consumes the nonce,
  then the replayed `Advance` presents the stale value and the whole transaction
  reverts. Cancellation is therefore itself a transaction file with
  an empty inner message on the same nonce account. Submitting it does nothing except consume
  the nonce. `nonce advance` derives the nonce account and expected nonce from the file
  being revoked and emits the cancellation file, which walks the normal
  inspect → sign → submit loop.

The cancellation and the original transaction file both present the same stored
nonce, and exactly one can land. Nothing is revoked until the cancellation
confirms, so keep the original file from being submitted only after the
cancellation has landed.

## Flow patterns

**Parallel submission, two nonce accounts.** One nonce account is one serial queue, so two transaction files on
the same nonce account exclude each other by design. When two transactions must both land, give
each its own nonce account: create two nonce accounts under the same authority, create each
transaction file against its own nonce account, and move both files to the cold side together. The
signer inspects and signs each file in a single air-gap session, and the relayer
submits both in any order, even in the same block, because each consumes a different
nonce. Nonce accounts are cheap (rent on 64 bytes) and long-lived, so the working pattern is a
standing pool per authority sized to the parallelism you want, not nonce accounts created per
transaction. `transaction sign` accepts multiple files so a batch is one signing session.

**Designated relayer, a required outer signature.** Anyone with a fully signed
transaction file can submit it until its nonce account advances. A cold signer
who wants submission control names a submit signer at
transaction-create time. The source CLI must put that key in the inner message
first, usually as a fee-payer-shaped signer or another command-specific signer. Then
`transaction create --submit-signer <ADDRESS>` validates that the dumped message already
requires that key and also requires the key on the wrapped transaction. The file
cannot reach fully-signed until the submit signer adds an ordinary Ed25519 signature to
it, online, without a separate transaction file of their own. And because the signer program forwards
outer privilege only for wrapped-transaction signers who also sign the outer `Submit`
transaction, the executor refuses the replay unless the submission itself carries the
submit signer's live signature. A leaked transaction file is inert in anyone else's hands.
Authorization stays with the cold signer, timing and sequencing stay with the designated relayer,
which is typically the fee payer so the outer signature comes for free.

## Repository layout

On-chain code and its per-program helper crates stay in the existing
program-specific top-level directories. Everything intended as a combined consumer
surface lives under `clients/`, which is also the dependency boundary: `clients/`
may depend on the per-program crates, never the reverse. The per-program client
crates remain beside their programs because programs consume them on-chain as CPI
helpers, so they are building blocks, not combined consumer surfaces.

```text
executor/{interface,program,client}   message-executor wire contracts + on-chain code
nonce/{interface,program,client}      nonce wire contracts + on-chain code
signer/{interface,program,client}     signer wire contracts + on-chain code
clients/rust       Rust helpers
clients/cli        psigner CLI
clients/js         planned TS library
clients/web        planned reference coordinator app (future; do not create yet)
```

The hosted coordination service and the Ledger clear-signing plugin are separate
repos when they exist. The first has an operations lifecycle, the second belongs to
Ledger's build system.

## Rust Helpers: `clients/rust`

One new crate above the three per-program client crates (which stay as thin instruction
builders). Working name `spl-programmatic-signer-rust`; bikeshed separately.

**Core principles**: offline-first (no RPC dependency in the core), `Signers`-generic
(keypair, Ledger via remote-wallet, anything), wasm32-compatible core (no tokio, RPC
behind a feature), every fallible construction returns the typed errors we built.

```text
rust
├── sign_only.rs   (M1) Import Solana/SPL Token CLI
│                  `--sign-only --dump-transaction-message --output json-compact`
│                  JSON, decode its base64 message, and validate that the JSON
│                  blockhash matches the dumped transaction message.
├── transaction_plan.rs      (M1) Lower-level helper: a Vec<Instruction> + authority set + nonce account +
│                  optional submit signers who must countersign submission (see
│                  Flow patterns). Convenience constructors: transfer,
│                  arbitrary instructions.
├── transaction.rs (M1) Cold-signed transaction operations: build from imported
│                  VersionedMessage or lower-level TransactionPlan, sign (add
│                  signature via any Signer), merge, and signer status.
├── inspect.rs     (M1 basic, Planned decoders) Transaction file -> structured summary:
│                  executor program, nonce account, signer status, inner message,
│                  and the inner message's instruction/account list.
│                  The trust surface. Decoder priority: system first, then SPL
│                  Token/Token-2022, then memo/compute-budget/ATA, with a loud
│                  raw-bytes fallback for unknown programs. The hard problem of
│                  this whole product is inspection, not signing.
├── verify.rs      (M1) Machine preflight mirroring the on-chain checks, so a broken
│                  transaction file fails before anyone signs: the cold-signed
│                  transaction's lifetime specifier equals the target cluster
│                  genesis hash, the inner message's lifetime specifier equals the nonce
│                  account's stored nonce, the nonce authority is in the inner
│                  message's signer prefix, no lookup tables, no duplicate keys,
│                  exactly one executor instruction, and every program id (signer,
│                  executor, nonce) matches the configured deployment. Runs
│                  offline given a nonce snapshot and genesis hash; the relayer
│                  runs it again plus signature checks before paying fees.
├── submit.rs       (M1 offline assembly, CLI RPC submit) Fully signed transaction file -> hot relay transaction. Fee payer is
│                  the relayer's signer. The CLI verifies live nonce state by default,
│                  submits, confirms, and reports the advanced nonce. Typed program
│                  error decoding and compute-budget derivation are follow-on relayer UX work.
├── nonce.rs       (M1) Nonce lifecycle: create+initialize (atomic pair), fetch state,
│                  derive next nonce locally for display, direct advance instruction
│                  for keypair authorities, empty-inner-message cancellation file for PDA
│                  authorities (see Nonce lifecycle).
├── pda.rs         (M1) ProgrammaticSigner derivation, re-exported.
└── error.rs       (M1) Typed errors and program failure decoding.
```

The `execute`/`wrapped_message`/`submit` building blocks already exist in the program
client crates; the Rust helpers compose them and own the transaction-file layer
only. RPC-backed simulation currently lives in the CLI rather than the Rust helper
crate.

## CLI: `clients/cli`, binary `psigner`

Mirrors `solana` CLI idioms (keypair paths, `--url`, output formats) so cold-custody
operators reuse existing habits. The MVP does not invent a transfer DSL. It imports
the sign-only transaction message that `solana`, `spl-token`, or an application CLI
already knows how to build:

```text
# Build the inner message with the Solana CLI and dump it as sign-only JSON.
solana ... --blockhash <NONCE> --sign-only --dump-transaction-message --output json-compact

# Build the inner message with the SPL Token CLI and dump it as sign-only JSON.
spl-token ... --blockhash <NONCE> --sign-only --dump-transaction-message --output json-compact
```

The resulting JSON's `message` field is the base64 encoded transaction message that
`psigner transaction create --from-sign-only` wraps. The upstream `signers` and
`absent` fields are diagnostic only; `badSig` rejects import because the source CLI
reported a signature that did not verify. Wrapper signatures are collected later by
`psigner transaction sign`. The coordinator uses `psigner address <AUTHORITY>` before
the upstream command and supplies that ProgrammaticSigner PDA anywhere the inner
transaction needs signer privilege. `psigner address` is a Solana-style utility
command; the stateful surfaces stay under `nonce` and `transaction`. For SPL Token
multisig, that usually means member PDAs in repeated `--multisig-signer` flags and the
multisig account itself in `--owner`; for single-authority token or SOL flows, it
usually means the key passed to the source CLI as `--owner`, `--from`, or `--fee-payer`.
A `--submit-signer` must also be present as a required signer in the dumped inner
message, often by setting the source CLI's `--fee-payer <SUBMIT_SIGNER_PUBKEY>`.
`psigner` rejects submit signers that the source CLI did not put in the message.

```text
# Derive and print the ProgrammaticSigner PDA for a cold authority.
psigner address <AUTHORITY>

# Create and initialize one SPL Nonce account for an authority.
psigner nonce create
        (--programmatic-authority <COLD_AUTHORITY> | --nonce-authority <ADDRESS>)
        --nonce-keypair nonce.json
        --fee-payer payer.json

# Show the current nonce value, authority, and account balance.
psigner nonce show <NONCE_ACCOUNT>

# Build a PDA-authority cancellation file from an existing transaction file.
psigner nonce advance
        --from-transaction tx.psigner
        --authority <COLD_AUTHORITY>
        --outfile advance.psigner

# Build a PDA-authority cancellation file from explicit nonce account inputs.
psigner nonce advance
        --nonce <NONCE_ACCOUNT>
        --authority <COLD_AUTHORITY>
        (--nonce-value <HASH> | --fetch-nonce)
        [--genesis-hash <HASH> | --fetch-genesis-hash]
        --outfile advance.psigner

# Wrap a Solana CLI or SPL Token CLI sign-only message dump into a transaction file.
psigner transaction create
        --from-sign-only inner.json
        --nonce <NONCE_ACCOUNT>
        --authority <AUTHORITY>...
        [--submit-signer <ADDRESS>...]
        [--genesis-hash <HASH> | --fetch-genesis-hash]
        [--fetch-nonce | --nonce-value <HASH> [--nonce-authority <ADDRESS>]]
        --outfile tx.psigner

# Decode the transaction file, nonce account, signer status, and inner message for offline review.
psigner transaction inspect tx.psigner

# Add one cold-authority signature to the transaction file.
psigner transaction sign tx.psigner
        --keypair <KEYPAIR_OR_URL>
        --outfile tx.signed.psigner

# Combine signatures collected on separate copies of the same transaction file.
psigner transaction merge <FILES...>
        --outfile tx.merged.psigner

# Sign several transaction files in one session and write signed copies to a directory.
psigner transaction sign tx1.psigner tx2.psigner
        --keypair <KEYPAIR_OR_URL>
        --outdir signed/

# Run static checks offline; with RPC, also check the live nonce account and genesis hash.
psigner transaction verify tx.psigner
        (--nonce-value <HASH> --nonce-authority <ADDRESS> | --fetch-nonce)
        [--genesis-hash <HASH> | --fetch-genesis-hash]

# Simulate the inner message before signing.
psigner transaction simulate inner tx.psigner

# Build and simulate the hot relay transaction after signing.
psigner transaction simulate relay tx.signed.psigner --fee-payer <KEYPAIR_OR_URL>

# Assemble the hot relay transaction, send it, and confirm it.
psigner transaction submit tx.psigner
        --fee-payer <KEYPAIR_OR_URL>
        [--submit-signer <KEYPAIR_OR_URL>]
        [--blockhash <HASH>]
        [--genesis-hash <HASH> | --fetch-genesis-hash]
        [--no-send --outfile submit.tx]
        [--skip-verify]
```

`--outfile` is intentionally separate from Solana's `--output json|json-compact`
display-format flag. `transaction inspect` and `transaction sign` must work without
RPC so the cold side can be fully air-gapped. The runbook is build upstream sign-only
message → `transaction create` → move the file → **inspect on the signing machine** →
sign → move the file → submit.

CLI signer arguments accept local Solana keypair files and Solana remote-wallet URLs
such as `'usb://ledger?key=0'`. This applies to `--keypair`, `--fee-payer`, and
`--submit-signer`; `--nonce-keypair` stays file-only because it creates the new nonce
account address. Quote hardware-wallet URLs in shells like zsh. Ledger may require
blind signing for this wrapped transaction format, so the signing runbook still starts
with `transaction inspect` on the cold machine.

`transaction inspect` decodes System Program transfers and otherwise shows program id,
account indexes, resolved account keys, and base64 instruction data. SPL Token,
Token-2022, Memo, Compute Budget, and ATA semantic decoders are the next inspection
priority before broad cold-signer rollout.

## Web stack

Two packages, TS-first, WASM only as a fallback.

**`@solana-program/programmatic-signer` (TS library).** Thin codecs (tag byte +
kit's transaction codecs) implementing the same transaction-file and inspect API as the
Rust helpers, on `@solana/kit`. Cold signing in the browser is a standard
`signTransaction` wallet request, which every wallet adapter already supports. Fixture
coverage should use the same real Solana/SPL Token sign-only JSON shapes as the Rust
helpers, plus TS/Rust round-trip checks for transaction files.
If the codec surface grows beyond "tag + standard wire", revisit compiling the Rust
core to WASM instead; do not maintain two nontrivial serializers.

One capability constraint kit's signer taxonomy makes explicit: the cold-signing flow
requires a wallet that can *sign without sending* (`signTransaction`, not only
`signAndSendTransaction`). The library should detect and reject send-only wallets with
a clear message rather than letting them fail downstream.

**Reference web app (transaction coordinator).** Single-page, no backend required, and
the signing view must function with zero RPC access (pure decode plus wallet sign), so
an air-gapped device with a browser wallet can serve as the cold side:
build transaction (guided transfer form + raw mode), live decoded preview, QR/file
import-export of transaction files, signature collection status, wallet-based signing, submit
with any connected wallet as fee payer. Doubles as the `inspect` surface for people
who will not install a CLI. A hosted coordination service (share a transaction file by link,
notify signers) is a later, optional layer; the file/QR flow must stand alone first.

## Simulation

People will want to simulate before anything costs money or a signature. Three stages,
one principle: simulation is advisory, its results are never embedded in the transaction file,
and nothing downstream trusts it.

**Stage 1, static (offline).** `verify`, described above. No RPC, no signatures.

**Stage 2, inner transaction simulation (before any signature exists).** The relay transaction
path cannot be simulated pre-signing: the signer program verifies real Ed25519 signatures inside the
instruction data, where RPC's `sigVerify: false` cannot reach. But the inner message
is itself a well-formed message, so the Rust helpers simulate it directly as its own
transaction with `sigVerify: false` and `replaceRecentBlockhash: true`. That answers
the question people are actually asking, "what will this do", with RPC error status,
compute units, and logs against live state. `transaction simulate inner` runs this explicitly
when online, and a future web coordinator can add balance-change rendering next to the decoded transaction. Two
fidelity caveats, both stated in output: it exercises the inner message but not the
signer/executor/nonce plumbing, and the inner message standalone has more CPI depth headroom than it
will have through the relay path, so a depth-heavy inner program can pass stage 2 and still
fail on-chain.

**Stage 3, dress rehearsal (after signatures are collected).** Once the transaction file is
fully signed, the real hot relay transaction simulates end-to-end through all
three programs, signature verification included. This is a true rehearsal.
`transaction simulate relay <TRANSACTION_FILE> --fee-payer <KEYPAIR>` runs it explicitly before
submit. Automatic pre-submit simulation and compute-budget derivation are follow-on
relayer UX work, not part of the first CLI binary.

**TODO, effects preview**: an `--effects` mode on `transaction simulate relay` that
answers "what will this change" instead of "did the VM return ok". Implementation
shape: derive the hot relay transaction's writable accounts, fetch pre-state with
`getMultipleAccounts`, request post-simulation snapshots through
`simulateTransaction`'s `accounts` parameter, then diff lamports, owner, and data,
decoding known accounts through the same decoder ladder `inspect` uses (System first,
then Token/Token-2022) and printing the nonce account as `nonce: <old> -> <new>`.
Output carries a simulated-at-slot line because pre-state and simulation are not
atomic. The sibling follow-on prints landed effects on `transaction submit`
confirmation from `getTransaction` metadata, which is definitive rather than
advisory. This is a security surface, not sugar: a designated submit signer running
the effects preview sees an inner message that spends their own key before they sign
it, which structural `verify` cannot catch. No third-party preview services in the
core, and raw JSON output stays available.

**Advanced tier, later**: a local SVM twin (LiteSVM-style) that fetches the referenced
accounts, substitutes ephemeral authorities and a twin nonce account, and runs the
relay path locally before any real signature. Relay-path fidelity pre-signing, at the
cost of real machinery. Optional Rust-helper feature, not a launch dependency.

**Division of trust**: simulation informs the coordinator and the relayer, who are
online. The cold signer is offline by design and cannot simulate; their tool is
inspection. The runbook keeps these straight rather than pretending the cold
side can rehearse.

## Cross-cutting

- **Compatibility fixtures**: real Solana/SPL Token sign-only JSON plus transaction-file
  round-trip tests keep the import contract honest without making a separate fixture
  package part of the MVP.
- **Error UX**: submit currently surfaces RPC errors and reports the confirmed
  signature plus advanced nonce. Decoding `Custom(n)` against the three programs'
  enums and printing the variant name/doc line is follow-on relayer UX work.
- **Nonce concurrency**: the Rust helpers treat one nonce account as one serial queue and make
  multiple nonce accounts per authority the documented pattern for
  parallel transaction files.
- **Depth budget**: the stack consumes CPI depth before the inner message runs. The
  MVP inspector shows raw program ids and instruction data; program-specific depth
  warnings are follow-on inspection work.
- **TODO, inner-instruction compatibility matrix**: document and test which
  instruction classes survive CPI replay unchanged. Known divergence classes to
  verify first: precompile instructions cannot be invoked via CPI, ComputeBudget
  instructions are transaction-level, and instructions-sysvar introspection inside
  a replayed program observes the hot relay transaction rather than the inner
  message. Every row needs a test before it is documented.
- **TODO, documentation checks in CI**: snapshot the CLI help against the
  documented command tree, smoke-test the quickstart mainline against a local
  validator, and add a link check. The Rust doc-comment spellchecker does not
  cover markdown.

## Delivery plan

Nothing is published to crates.io or npm during these milestones, and deployment is
deliberately the last milestone. Consumers work from the repo (see Local consumption).

| Milestone       | Deliverable                                                                                                                                                                                                           | Done when                                                                                     |
|-----------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------------------------|
| M1 Rust helpers | `clients/rust` core (sign-only import, arbitrary-message transaction construction, transaction-plan helper, verify, nonce, pda, submit assembly), real sign-only fixtures, every flow-matrix row tested with Mollusk  | Flow matrix fully green in CI, fixture import/inspect/verify tests pass                       |
| M2 CLI          | `clients/cli` (`psigner`) on the Rust helpers, offline transaction-file commands, RPC-backed nonce and submit commands, local-validator harness                                                                       | Offline CLI tests pass and localnet RPC tests are available behind an ignored harness         |
| M3 TS library   | `clients/js` validated against real sign-only fixtures and transaction-file round trips                                                                                                                               | TS tests prove compatibility with the Rust helpers and source CLI output                      |
| M4 Web app      | `clients/web` reference coordinator                                                                                                                                                                                   | Signing view works with zero RPC access                                                       |
| M5 Deployment   | Immutable production deployment: upgrade authority removed after audit, program address registry, deploy keypair custody. A historical July 2026 devnet deployment exists, but its pre-rebase ids and wire format are incompatible with this branch. | Stable program ids published, upgrade authority removed, and the quickstart runs against them |

Later, unscheduled: coordination service, Ledger clear-signing plugin, additional
signer schemes as sibling programs appear.

Docs land alongside M1: `docs/architecture.md` (how the three programs compose, the
security model and trust boundaries, comparison with system durable nonces, benefits,
and expectations for use including pre-audit status) and `docs/migration.md` (the
durable-nonce migration story: concept mapping, command mapping, what client support
migration needs).

## Flow coverage matrix

The forcing function for M1 and M2: every documented flow has Rust-helper support and
Mollusk coverage, and the CLI has deterministic offline coverage for the transaction
handoff path plus an ignored local-validator harness for RPC-backed nonce flows. A
flow without a passing test does not ship.

| Flow | Documented in | Rust test | CLI test |
|------|---------------|-----------|----------|
| Open a nonce account | Nonce lifecycle, setup | `nonce_account_setup` | ignored `nonce_create_and_show_against_local_validator` |
| Basic transaction file, create → sign → submit | The three roles | `transaction_end_to_end` | `offline_transaction_flow_uses_sign_only_json_as_input` |
| Replay rejected after consumption | Nonce lifecycle, consumption | `replay_rejected` | local-validator harness planned after full submit fixture funding |
| Cancellation, keypair authority | Nonce lifecycle, cancellation | `cancel_keypair_authority` | local-validator harness planned after direct-advance CLI shape |
| Cancellation, PDA authority | Nonce lifecycle, cancellation | `cancel_pda_authority` | `nonce_advance_from_transaction_builds_cancellation_transaction` |
| Parallel submission, two nonce accounts | Flow patterns | `parallel_nonce_accounts` | covered by batch `transaction sign` shape; local-validator landing test planned |
| Designated relayer, outer signature | Flow patterns | `designated_relayer` | covered by `transaction submit --submit-signer` command path; local-validator landing test planned |

The designated-relayer row includes the negative case: a submission without the
submit signer's outer signature must fail.

## Local consumption

There are no published packages. A historical devnet test deployment exists
(program ids in the quickstart), but it is not compatible with the current
execution-program design. Consumers work from the repo directly:

- Rust: git dependencies on `clients/rust` (and the per-program client crates as needed).
- CLI: `cargo run -p psigner --` or `cargo install --path clients/cli`.
- Tests and examples need the program binaries first: `make build-sbf-nonce-program
  build-sbf-signer-program build-sbf-executor-program`, then run
  with `SBF_OUT_DIR=$PWD/target/deploy`.
- Pre-1.0 expectations, stated plainly: wire formats and program bytes will
  change, transaction files should not be kept across test deployment upgrades,
  and the programs are unaudited.

## Open questions

- Crate/package/binary names (`spl-programmatic-signer-rust`, `psigner`,
  `@solana-program/programmatic-signer` are placeholders).
- Whether wallets' simulation UX complains about the cold-signed transaction (genesis-hash blockhash, unknown
  program) in browser signing; needs empirical testing per wallet early in phase 3.
- QR chunking format for transaction files above one code (adopt an existing multi-part QR
  scheme rather than inventing one).
- Whether `transaction submit` should support address lookup tables in the *outer*
  transaction for account-heavy flows (the inner message cannot use them by design).
