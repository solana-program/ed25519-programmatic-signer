# Client and tooling plan

How people actually use the signer, executor, and nonce programs: a Rust SDK, a CLI,
and a web stack that let anyone act as the cold signer, the coordinator, or the hot
relayer without touching wire formats by hand.

## Research anchors

Four facts shape every decision below.

**1. The wrapped message is a standard Solana transaction.** The artifact a cold wallet
signs (message A) is a legacy `VersionedTransaction`, byte-for-byte the format the whole
ecosystem already signs and moves around. Ledger signs it via `solana-remote-wallet`.
Browser wallets sign it via the standard `signTransaction` request. `@solana/kit`
already partial-signs, serializes to base64 wire format, and deserializes it. We do not
need to invent a transport format for the signing ceremony. We need to wrap an existing
one.

**2. Solana already has an offline-signing idiom, and our users already know it.** The
CLI's durable-nonce workflow (`--sign-only`, `--blockhash <nonce>`, signer pairs on
stdout, `--signer` on the online side) is the established muscle memory for air-gapped
ceremonies. This system is philosophically "durable nonces generalized to program-level
messages", so the CLI should feel like those commands, not like a new paradigm.

**3. wincode is bincode-compatible.** The instruction data our interfaces define is a
one-byte tag followed by standard-wire-format message/transaction bytes. A TypeScript
client therefore needs no wincode port and no WASM bridge for the core path: kit's
existing transaction codecs plus a tag byte reproduce our instruction data. This must be
locked with golden vectors, not assumed.

**4. Hardware wallets display our payload as an opaque blob.** A Ledger shows message A
as "unknown program, N accounts, data bytes". The security of the ceremony therefore
rests on tooling-level verification: every surface must be able to decode an artifact
back to human-readable intent ("transfer 5 SOL from your PDA to X, consuming nonce N"),
and the recommended ceremony verifies on a second device. A Ledger clear-signing plugin
is a future workstream, not a launch dependency.

## The three roles

```text
  COORDINATOR (online)            COLD SIGNER (air-gapped)         RELAYER (online, pays)
  ────────────────────            ────────────────────────         ──────────────────────
  reads nonce, builds intent      verifies decoded intent          verifies signatures
  compiles B, wraps into A   ──▶  signs A (Ledger/keypair/    ──▶  assembles outer tx
  emits ceremony artifact         browser wallet), re-emits        submits, confirms,
                                  artifact with signature          reports typed errors
```

One person can play all three roles (hot-wallet convenience flow), or three different
parties on three machines (the target cold-custody flow). Multi-authority ceremonies
fan the middle step out to several signers who each add a signature to the same
artifact; signatures are position-independent so collection order does not matter.

## The ceremony artifact

The single interop point. A versioned JSON envelope around the standard wire format:

```json
{
  "version": 1,
  "genesis_hash": "<cluster genesis hash, unambiguous across custom clusters>",
  "transaction": "<base64 standard wire format of A, partially signed>",
  "note": "optional human memo, not signed, display only"
}
```

- The `transaction` field alone is sufficient; everything else is convenience. Any
  existing tool that understands partially-signed Solana transactions can carry it.
- The transaction bytes are canonical. The envelope deliberately carries no redundant
  copies of the inner message or executor instruction: everything for display is
  derived by decoding, and nothing on the signing side ever re-serializes. Redundant
  copies would create a consistency surface that then needs its own verification.
- Decoding for display derives everything else: the executor instruction, inner message
  B, the nonce account and expected nonce, per-instruction summaries.
- Transports: file (`.ceremony.json`), stdin/stdout pipes, QR code chunks for air-gap
  (fits comfortably: one instruction B ≈ 400-700 bytes).
- Signatures accumulate in the standard signature slots. "Fully signed" is checkable
  offline (every required slot non-default).

## Nonce lifecycle

A lane has exactly three moves: open it once, consume it on every executed ceremony,
and advance it manually to revoke anything signed but not yet landed.

**Setup, once per lane.** `nonce create` sends one transaction: a system
`create_account` funding rent for the 64-byte state with the SPL Nonce program as
owner, then `Initialize`, which binds the authority and derives the first nonce from
the account address and a recent slot hash. The authority is a plain readonly account
in `Initialize` and never signs setup, so any funded wallet can open lanes for any
authority and provisioning never touches cold keys. `nonce show` reads back the lane
to hand to a coordinator. Ceremonies that must not compete get their own lanes (see
Nonce concurrency).

**Consumption, every relay.** Executing a ceremony consumes its nonce: the executor's
final step CPIs `Advance`. A landed relay therefore invalidates the artifact, every
copy of it, and every other outstanding artifact on the same lane, atomically with the
intent it executed. `relay` reports the advanced nonce on confirmation. When two
artifacts race one lane, exactly one lands and the other dies with `NonceMismatch`.

**Cancellation, revoking a signed artifact.** Signatures cannot be recalled, so
revocation means consuming the lane's nonce before the unwanted artifact lands. The
flow depends on who the nonce authority is.

- Keypair authority: one direct `Advance` transaction. The authority signs it and
  presents the stored nonce. `nonce advance` builds, sends, and confirms it.
- PDA authority: the PDA cannot sign a transaction on its own, and wrapping `Advance`
  inside the inner message self-destructs: the replayed `Advance` consumes the nonce,
  then the executor's own final `Advance` presents the stale value and the whole
  transaction reverts. Cancellation is therefore itself a ceremony with an empty
  intent, an inner message with no instructions on the same lane. Relaying it does
  nothing except consume the nonce. `cancel` derives the lane and expected nonce from
  the artifact being revoked and emits the cancellation artifact, which walks the
  normal inspect → sign → relay loop.

A cancellation races whatever it revokes. Both present the same stored nonce and
exactly one can land, so nothing is revoked until the cancellation confirms. Cancel
first, then stand down the relayer, not the other way around.

## Rust SDK: `crates/sdk`

One new crate above the three per-program client crates (which stay as thin instruction
builders). Working name `spl-programmatic-signer-sdk`; bikeshed separately.

**Core principles**: offline-first (no RPC dependency in the core), `Signers`-generic
(keypair, Ledger via remote-wallet, anything), wasm32-compatible core (no tokio, RPC
behind a feature), every fallible construction returns the typed errors we built.

```text
sdk
├── intent.rs      Intent: a Vec<Instruction> + authority set + nonce account.
│                  Convenience constructors: transfer, arbitrary instructions.
├── ceremony.rs    Ceremony artifact: build (Intent + nonce value -> unsigned A),
│                  sign (add signature via any Signer), merge (combine artifacts),
│                  status (which authorities have signed), encode/decode envelope.
├── inspect.rs     Artifact -> structured summary: executor program, nonce + account,
│                  B's instructions decoded, writable/signer grants per account.
│                  The trust surface. Decoder priority: system first, then SPL
│                  Token/Token-2022, then memo/compute-budget/ATA, with a loud
│                  raw-bytes fallback for unknown programs. The hard problem of
│                  this whole product is inspection, not signing.
├── verify.rs      Machine preflight mirroring the on-chain checks, so a broken
│                  ceremony fails before anyone signs: B's lifetime equals the
│                  nonce account's stored nonce, the nonce authority is in B's
│                  signer prefix, no lookup tables, no duplicate keys, exactly
│                  one executor instruction in A, and every program id (signer,
│                  executor, nonce) matches the configured deployment. Runs
│                  offline given a nonce snapshot; the relayer runs it again
│                  plus signature checks before paying fees.
├── simulate.rs    Feature "rpc". Stage-2 intent simulation and stage-3 dress
│                  rehearsal (see Simulation), returning balance deltas, logs,
│                  compute units, and typed-error decoding.
├── relay.rs       Fully-signed artifact -> outer Submit transaction. Fee payer is
│                  the relayer's signer. Feature "rpc": simulate by default,
│                  derive the compute budget from it, submit + confirm + decode
│                  program errors back to typed enums, report the advanced nonce.
├── nonce.rs       Nonce lifecycle: create+initialize (atomic pair), fetch state,
│                  derive next nonce locally for display, direct advance for
│                  keypair authorities, empty-intent cancellation ceremony for
│                  PDA authorities (see Nonce lifecycle).
└── pda.rs         ProgrammaticSigner derivation, re-exported.
```

The `execute`/`wrapped_message`/`submit` building blocks already exist in the program
client crates; the SDK composes them and owns the artifact/ceremony layer only.

## CLI: `crates/cli`, binary `psigner`

Mirrors `solana` CLI idioms (keypair paths, `--url`, output formats) so cold-custody
operators reuse existing habits. Command tree:

```text
psigner pda <AUTHORITY>                     derive + show PDA, balance
psigner nonce create --authority <PDA>      create_account + Initialize, one tx
psigner nonce show <NONCE_ACCOUNT>          current nonce, authority
psigner nonce advance <NONCE_ACCOUNT> --authority cold.json
                                            keypair authority consumes the nonce
                                            directly, revoking outstanding artifacts
psigner propose transfer --from-pda ... --to ... --lamports ...
psigner propose raw --instructions ix.json  arbitrary instruction list
        --nonce-account <ADDR> [--nonce <VALUE>]   (value flag = fully offline)
        -o ceremony.json | --qr
psigner inspect ceremony.json               decoded intent, signer status; the
                                            command you run on the second device
psigner sign ceremony.json --signer usb://ledger | --keypair cold.json
                                            offline capable, re-emits artifact
psigner cancel ceremony.json                emit an empty-intent ceremony on the same
                                            lane, revoking this artifact once signed
                                            and relayed (PDA authorities)
psigner verify ceremony.json                machine preflight + signature status,
                                            what the relayer runs before paying
psigner simulate ceremony.json              unsigned artifact: intent simulation,
                                            signed artifact: full-stack rehearsal
psigner relay ceremony.json --fee-payer hot.json
                                            assemble Submit, send, confirm
```

The propose/sign/relay split is deliberately isomorphic to the durable-nonce
`--sign-only` / `--signer` flow. `inspect` is non-optional in the docs: the ceremony
runbook is propose → move → **inspect on the signing machine** → sign → move → relay.

## Web stack

Two packages, TS-first, WASM only as a fallback.

**`@solana-program/programmatic-signer` (TS library).** Thin codecs (tag byte +
kit's transaction codecs) implementing the same artifact/inspect/ceremony API as the
Rust SDK, on `@solana/kit`. Cold signing in the browser is a standard
`signTransaction` wallet request, which every wallet adapter already supports. Golden
vectors shared with the Rust repo make divergence a CI failure rather than an incident.
If the codec surface grows beyond "tag + standard wire", revisit compiling the Rust SDK
core to WASM instead; do not maintain two nontrivial serializers.

One capability constraint kit's signer taxonomy makes explicit: the cold-signing flow
requires a wallet that can *sign without sending* (`signTransaction`, not only
`signAndSendTransaction`). The library should detect and reject send-only wallets with
a clear message rather than letting them fail downstream.

**Reference web app (ceremony coordinator).** Single-page, no backend required, and
the signing view must function with zero RPC access (pure decode plus wallet sign), so
an air-gapped device with a browser wallet can serve as the cold side:
build intent (guided transfer form + raw mode), live decoded preview, QR/file
import-export of artifacts, signature collection status, wallet-based signing, relay
with any connected wallet as fee payer. Doubles as the `inspect` surface for people
who will not install a CLI. A hosted coordination service (share ceremony by link,
notify signers) is a later, optional layer; the file/QR flow must stand alone first.

## Simulation

People will want to simulate before anything costs money or a ceremony. Three stages,
one principle: simulation is advisory, its results are never embedded in the artifact,
and nothing downstream trusts it.

**Stage 1, static (offline).** `verify`, described above. No RPC, no signatures.

**Stage 2, intent simulation (before any signature exists).** The full stack cannot be
simulated pre-signing: the signer program verifies real Ed25519 signatures inside the
instruction data, where RPC's `sigVerify: false` cannot reach. But the inner message B
is itself a well-formed message, so the SDK simulates it directly as its own
transaction with `sigVerify: false` and `replaceRecentBlockhash: true`. That answers
the question people are actually asking, "what will this do", with balance deltas,
logs, and program errors against live state. `propose` runs it by default when online,
and the web coordinator renders its balance changes next to the decoded intent. Two
fidelity caveats, both stated in output: it exercises the intent but not the
signer/executor/nonce plumbing, and B standalone has more CPI depth headroom than it
will have through the stack, so a depth-heavy inner program can pass stage 2 and still
fail on-chain (inspect's depth warning is the guard).

**Stage 3, dress rehearsal (after signatures are collected).** Once the artifact is
fully signed, the real outer `Submit` transaction simulates end-to-end through all
three programs, signature verification included. This is a true rehearsal. `relay`
runs it by default before paying fees, derives the compute budget from its measured
units, and decodes failures to typed error variants. `--skip-simulation` exists for
the impatient.

**Advanced tier, later**: a local SVM twin (LiteSVM-style) that fetches the referenced
accounts, substitutes ephemeral authorities and a twin nonce account, and runs the
full stack locally before any real signature. Full-stack fidelity pre-signing, at the
cost of real machinery. Optional SDK feature, not a launch dependency.

**Division of trust**: simulation informs the coordinator and the relayer, who are
online. The cold signer is offline by design and cannot simulate; their tool is
inspection. The ceremony runbook keeps these straight rather than pretending the cold
side can rehearse.

## Cross-cutting

- **Golden vectors**: one JSON fixture set (intent → B bytes → A bytes → Submit data)
  generated by the Rust SDK, consumed by Rust tests, TS tests, and CLI snapshot tests.
  This is the contract that keeps three implementations honest.
- **Error UX**: relay decodes `Custom(n)` against the three programs' enums and prints
  the variant name and doc line. The pipeline-ordered errors were built for this.
- **Nonce concurrency**: the SDK treats one nonce account as one serial lane and makes
  multiple lanes (multiple nonce accounts per authority) the documented pattern for
  parallel ceremonies.
- **Depth budget**: inspect warns when B's instructions target programs known to CPI
  more than two levels deep (stack limit 5, three consumed by the stack; lifts when
  SIMD-0268 activates).

## Phasing

1. **Rust SDK core** (artifact, ceremony, inspect, nonce lifecycle) + golden vectors.
   Everything else consumes this.
2. **CLI** on the SDK, full propose/inspect/sign/relay loop against localnet, Ledger
   signing via remote-wallet. This is the first end-to-end usable product.
3. **TS library** validated against golden vectors, kit + wallet-adapter integration.
4. **Reference web app** on the TS library.
5. **Later**: coordination service, Ledger clear-signing plugin, additional signer
   schemes as sibling programs appear.

## Open questions

- Crate/package/binary names (`spl-programmatic-signer-sdk`, `psigner`,
  `@solana-program/programmatic-signer` are placeholders).
- Whether wallets' simulation UX complains about A (default blockhash, unknown
  program) in browser signing; needs empirical testing per wallet early in phase 3.
- QR chunking format for artifacts above one code (adopt an existing multi-part QR
  scheme rather than inventing one).
- Whether `propose` should support address lookup tables in the *outer* transaction
  for account-heavy ceremonies (inner B cannot use them by design).
