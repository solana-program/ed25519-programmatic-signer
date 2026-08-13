# Architecture

This document describes the three-program pipeline that enables program-level durable
nonces and programmatic signing.

The programs are small on purpose. The signer program verifies authority signatures
over one cold-signed transaction (the wrapped transaction, in program terms). The
executor program replays one inner message. The
nonce program owns the nonce account state and consumes the nonce. Security comes
from keeping those jobs separate and letting the Solana runtime carry signer and writable
privileges across CPI.

## The three-program pipeline

A submission flows through the three programs as a nested set of messages.

The relayer submits the hot relay transaction. It pays the network fee and calls the
SPL Ed25519 Signer program. Its instruction data wraps the cold-signed transaction, a
standard `VersionedTransaction`. The cold-signed transaction contains exactly one
executor instruction. The executor instruction carries the inner message as its
instruction data.

1. The signer program verifies the authority signatures on the cold-signed
   transaction. It then CPIs its single executor instruction.
2. The executor program unpacks the inner message and binds the runtime accounts to
   it. After validation, it invokes the nonce program's `Advance` instruction via CPI.
3. With the nonce consumed, the executor replays the inner message
   instruction-by-instruction via CPI. Transaction rollback restores the old nonce if
   any replayed instruction fails.

The inner message's lifetime specifier (the field legacy messages call
`recent_blockhash`) is not used as a cluster recent blockhash. It carries the
nonce account nonce. The executor checks that value against the nonce account's
stored nonce before replay. The nonce program checks the same value again when
`Advance` runs immediately before the replay.

The cold-signed transaction's lifetime specifier carries the cluster genesis
hash. This is a tooling convention, not a program check: the runtime never evaluates
the cold-signed transaction on its own, so the field is free to carry a cluster label
covered by the cold signatures. An offline signer can inspect it and a relayer can
verify it against RPC before submitting, while the programs treat it as opaque signed
bytes.

One transfer flow looks like this.

```text
  COORDINATOR (online)         COLD SIGNER (air-gapped)       RELAYER (online)
  ---------------------        -------------------------       ----------------
  reads nonce account state
  nonce = N
  cold authority = C

  builds the inner message
  with lifetime specifier = N,
  requiring C or C's
  ProgrammaticSigner

  wraps it in Execute, wraps
  Execute in the cold-signed tx,
  sets that tx's lifetime specifier
  to the genesis hash
  emits transaction file ----->  decodes the file
                              checks program ids
                              checks genesis hash
                              checks transfer details
                              checks nonce account and nonce N
                              checks required signers

                              signs the cold-signed tx
                              re-emits the file       ----->  checks signatures
                                                            fetches nonce account snapshot
                                                            checks still nonce N
                                                            assembles hot relay tx
                                                            pays outer fee

  ON CHAIN
  --------
  Signer verifies the cold-signed tx has exactly one executor instruction.
  Signer verifies every required Ed25519 signature over the cold-signed tx.
  Signer verifies Submit accounts mirror the cold-signed tx's static account keys.
  Signer promotes matching ProgrammaticSigner PDAs.
  Signer forwards only live outer signers that the cold-signed tx required.

  Executor verifies the nonce account, nonce program, and SlotHashes accounts.
  Executor reads nonce account state and verifies the inner lifetime specifier == N.
  Executor verifies the inner message is replayable with no duplicate account keys.
  Executor binds every inner message account to the matching runtime account.
  Executor verifies every required inner signer already has signer privilege.
  Executor verifies the nonce account authority is one of those required signers.

  Executor CPIs Nonce::Advance before replay to block recursive execution.
  Executor CPIs the transfer instruction from the inner message.

  Nonce verifies the stored authority is the signer presented by Executor.
  Nonce verifies current_nonce == N.
  Nonce stores the next nonce account nonce.
```

Each hop has a narrow verification job. The cold signer verifies the decoded
transaction before signing. The relayer verifies the transaction file is still submittable before
paying fees. The programs verify signatures, privileges, replay binding, and nonce
consumption in that order.

## Account and privilege model

The pipeline manages signer privileges across the nested messages.

Authorities sign the cold-signed transaction using standard Ed25519 signatures. In a
Solana message, the leading required signer account keys line up with the signature
slots. The signer program verifies each required signature over the serialized
message bytes. It rejects a cold-signed transaction with no required signatures.

The signer program receives one account for each static account key in the
cold-signed transaction, in the same order. It rejects missing accounts, extra
accounts, and key mismatches. The executor instruction is selected by the cold-signed
transaction itself. The `Submit` instruction does
not carry a separate executor program id that can disagree with the signed message.

Programmatic signer privilege is derived from the signer program id and the authority
address.

```text
ProgrammaticSigner = PDA("programmatic-signer", authority)
```

When an authority has signed the cold-signed transaction, the signer program derives that authority's
`ProgrammaticSigner` PDA. If the PDA appears in the executor instruction's account
list, the signer program invokes the executor with `invoke_signed` for that PDA. The
PDA therefore arrives at the executor with runtime signer privilege.

The signer program also forwards live outer signer privilege, but only for keys that
are both required signers of the cold-signed transaction and signers of the hot relay
transaction. This is what makes a designated relayer signature meaningful. The
cold-signed transaction can require the submit signer, and submission still fails
unless the hot relay transaction carries the submit signer's live signature.

The executor program maps the account references in the inner message one-to-one to the
runtime accounts provided after its three fixed accounts.

```text
Execute accounts
0  writable nonce account
1  SPL Nonce program
2  SlotHashes sysvar
3  inner message account key 0
4  inner message account key 1
...
```

It rebuilds the writability that the inner message assigns to each account. If the
inner message marks an account writable, the runtime account must actually be
writable. If it marks an account as a required signer, the runtime account must
actually be a signer. The
executor never manufactures signer privilege. It only forwards privilege that the
runtime already gave it.

The nonce account stores 64 bytes: a 32-byte nonce and a 32-byte authority address.
`Initialize` stores any authority address. The authority does not sign setup.
`Advance` requires the stored authority account to carry signer privilege and requires
the caller to present the currently stored nonce.

## Security model and trust boundaries

Programs explicitly do not verify constraints that are covered by atomicity or
downstream checks.

The executor never verifies Ed25519 signatures. It relies entirely on runtime signer
flags because required signers must already carry privilege from the hot relay transaction
or a signer program's PDA promotion.

The nonce match alone proves nothing. The nonce is public account data, so anyone can
copy it into a message. The stored authority among the required signers proves the
account owner authorized this replay and this nonce consumption.

Transaction atomicity covers the entire execution. If the nonce `Advance` or any
replayed instruction fails, the hot relay transaction reverts. The replayed
instructions and the nonce consumption land together or not at all.

The double-consumption guard in `Advance` re-checks the presented nonce. The executor
consumes it before replay, so a replayed instruction or recursive executor invocation
that tries to consume the same nonce sees a stale value and fails. If that failure
propagates, transaction rollback restores the original nonce.

Per-program trust boundaries are:

| Program | Verifies | Assumes from upstream | Deliberately never checks |
|---|---|---|---|
| SPL Ed25519 Signer | The cold-signed transaction sanitizes, has at least one required signature, and contains exactly one instruction. Every required signature verifies over it. Submit accounts match its static account keys. The instruction's account indexes hit static keys. | The outer Solana runtime has marked live outer signers correctly. The called program will interpret its instruction data and enforce replay policy. | It does not verify that the instruction targets the message executor. Executor-id pinning is client preflight plus what the authorities signed. It does not parse the inner message, read nonce state, simulate the transaction, or enforce replay protection. It is stateless. Replay protection belongs to the executor plus nonce pair. |
| SPL Message Executor | The nonce program id, nonce account owner, and SlotHashes sysvar are correct. The inner message's lifetime specifier equals the stored nonce account nonce. The inner message is legacy, v0 without address table lookups, or v1 with empty config. It sanitizes and has no duplicate account keys. Runtime accounts match its static keys, writability, and signer requirements. The nonce account authority is one of its required signers. | Signer privilege has already been provided by the runtime, either from the outer transaction or signer-program PDA promotion. Replayed programs enforce their own instruction semantics. | It does not verify Ed25519 signatures and does not inspect the business meaning of the inner message. Runtime signer flags are the security fact it needs, and CPI callees own their own validation. |
| SPL Nonce | `Initialize` verifies owner, length, zero-filled state, and rent exemption. `Advance` verifies ownership, initialized state, authority address, authority signer privilege, and exact nonce match. It stores the next nonce after a successful advance. | A caller that uses the nonce for replay protection has read the nonce account and decided when to consume it. The runtime will roll back the caller's work if `Advance` fails. | It does not inspect messages, signatures, fee payers, or the transaction. The nonce program is only a single-use nonce account state machine. |

The trust surface for users is different from the trust surface for programs. The
programs can prove that the signed bytes and runtime privileges line up. They cannot
prove that a human meant to sign those bytes. Inspection on the signing device is
therefore the primary security surface.

## System durable nonces comparison

This system generalizes durable nonces to program-level messages.

| Feature | System durable nonces | Programmatic pipeline |
|---|---|---|
| Replay domain | A whole Solana transaction uses the nonce as its transaction lifetime. | The inner message uses the nonce account nonce in its lifetime specifier. The hot relay transaction has its own normal lifetime. |
| Advance timing | The system nonce advance must be the transaction's first instruction. | The executor advances the nonce immediately before replay. If replay fails, the entire transaction—including the advance—rolls back. |
| Fee payer | The fee payer is part of the same transaction that the system nonce gates. | The relayer pays the hot relay transaction's fee. Cold authorities and PDAs do not need SOL for fees. |
| Nonce value | The system nonce is a durable transaction blockhash managed by the System Program. | The nonce account nonce is a program-derived hash over a tag, nonce account address, previous nonce, and the latest slot hash. |
| Authorities | A keypair authority signs the system nonce advance. | Any address that can carry runtime signer privilege may be the nonce account authority, including a keypair or any PDA. |
| Parallelism | Many system nonce accounts may share one authority. Each account gates one serial transaction. | Many SPL Nonce accounts may share one authority. Each nonce account gates one serial inner message. |
| Handoff | Offline flows move signer-pair strings for a transaction. | Offline flows move the cold-signed transaction's wire bytes as a transaction file. The current CLI stores them as bare base64 text, and cluster binding is signed into the transaction's lifetime specifier. |
| Composability | The nonce belongs to transaction loading and the System Program. | Replay protection is available to programs through CPI and composes with signer programs. |

The important difference is not that one side has only one nonce. Both models support
many nonce accounts per authority. The difference is what the nonce gates and when it
is consumed. A system durable nonce gates the whole transaction and is advanced first.
This pipeline gates the inner message and advances immediately before the inner work,
using transaction rollback to keep the advance and replay atomic.

## Benefits, expectations, and limitations

Benefits:

- Fee isolation. The relayer pays for the hot relay transaction. Cold keypairs and PDA
  treasuries do not need to hold SOL only to fund signatures or submissions.
- PDA authorities. A nonce account authority can be any address that can receive signer
  privilege. The signer program can promote authority-derived `ProgrammaticSigner`
  PDAs after the corresponding Ed25519 authority signs the cold-signed transaction.
- File portability. The signed payload is a standard Solana transaction. The
  current CLI stores it as base64 text, and existing signing and transport tools can
  carry the bytes while new tools decode them for inspection.
- Per-nonce account replay protection. One nonce account is one serial queue.
  Parallel flows use multiple nonce accounts under the same authority, and a landed
  submission invalidates outstanding transaction files on that nonce account.
- Program-level composability. The nonce is consumed through CPI, so replay
  protection can wrap inner program actions instead of only top-level transactions.

Expectations:

- The programs are unaudited.
- Pre-1.0 wire formats, program ids, and program bytes may change.
- New program ids and wire format changes invalidate in-flight transaction files.
  In-place upgrades do not, so land or rebuild outstanding files around test
  deployment upgrades.
- The July 2026 devnet deployment uses the pre-rebase program ids and wire format.
  It is not compatible with this branch. The execution-program ids declared by this
  checkout are not currently deployed on devnet. The final deployment will be immutable.
- Hardware wallets may show the cold-signed transaction as an opaque unknown-program payload until a
  clear-signing plugin exists.
- Inspection on the signing device is non-optional. The signing surface must decode
  the transaction file back to the human-readable transaction, nonce account, nonce,
  authority, and signer status before any cold signature is added.

Limitations:

- Address table lookups in the inner message are never resolved and are explicitly rejected.
- A v1 inner message is accepted only with an empty transaction config.
- Unknown future message versions are rejected.
- The inner message cannot contain duplicate static account keys.
- Every inner message account must be supplied in order at replay time. Required writable
  and signer privileges must already exist on the runtime accounts.
- The CPI depth budget is constrained and cluster-dependent. With the standard
  stack limit of 5 the pipeline consumes three levels, leaving depth two for
  programs called by the inner message. Clusters with SIMD-0268 active raise the
  limit to 9. It is inactive on the historical July 2026 devnet deployment.
