# Architecture

## Three programs and three messages

The cold authority is an ordinary Ed25519 public key. Its programmatic signer is a
PDA derived by the signer program. Assets and application authorities belong to
that PDA; the cold key authorizes its use through a signed wrapper message. This
does not convert a keypair account into a PDA or make the PDA a native wallet signer.

1. The **inner legacy message** contains the business instructions and uses the SPL
   nonce value in its recent-blockhash field. It can require one or more derived
   PDA signers and explicitly selected live signers.
2. The **wrapper message** contains exactly one current legacy-executor instruction.
   It embeds the inner message and binds its account list and privileges. Its
   required signatures belong to the cold authorities and designated submit signers.
   Its recent-blockhash field contains the selected cluster's genesis hash.
3. The **relay transaction** uses an ordinary fresh blockhash and online fee payer.
   The signer program verifies the wrapper signatures, derives the signer PDAs,
   and invokes the executor with those PDA signer privileges. The executor advances
   the nonce and executes the inner instructions atomically.

The high-level Rust client accepts `VersionedMessage` at API boundaries but currently
supports only legacy messages. V0 messages and address lookup tables are rejected.
It produces legacy wrappers and relays. The executor uses two fixed accounts, the
nonce account and nonce program, before the inner message accounts. Slot hashes are
used during nonce initialization, not as an executor account on every submission.

## Nonce state and transitions

An initialized nonce account has a strict 64-byte layout: nonce hash and authority.
The authority can be a keypair address at the low level, or a programmatic signer
PDA. The CLI's `--cold-authority` derives a PDA; `--nonce-authority` stores the supplied
address verbatim.

The successor nonce commits to the nonce program ID, nonce account address, current
nonce, and transition commitment derived from the exact serialized inner message.
`next_nonce` calls the current interface derivation helpers. The same inner message
has the same successor regardless of when it lands. Different competing messages
produce different successors. Changing wrapper signatures does not change the inner
message transition. A predicted successor is usable only after its predecessor succeeds.

Execution failure rolls back both business instructions and nonce advancement.
Cancellation is an empty inner message that consumes the current nonce. A chain can
be prepared offline, but only one competing transition from a given nonce can succeed.

## Validation boundaries

The client validates message shape, indices, current program IDs, required account
privileges, signature slots, nonce snapshots, and cluster genesis hash. Missing
signature slots are allowed during construction and merging; populated invalid
signatures are rejected. Submission requires every signature and checks the live
nonce and cluster before assembling the relay. The serialized relay must fit the
1232-byte network transaction limit. Arbitrary instruction support does not remove
compute, account, packet-size, or application-specific runtime limits.

Genesis binding is enforced by this client. The on-chain program does not obtain the
cluster genesis hash and independently compare it with the wrapper field. A custom
relayer must enforce the same policy; the wrapper field alone is not an on-chain
cross-cluster protection guarantee.

`simulate inner` recompiles the business instructions with the configured online
fee payer and disables signature verification. It helps diagnose instruction errors
but does not check nonce consumption or programmatic signature verification.
`simulate relay` validates and simulates the entire signed path. Simulation does not
reserve state, so competing writes can still make later submission fail.

A designated submit signer must appear in the inner message, sign the cold wrapper,
and sign the live relay. Merely choosing a hot fee payer does not restrict who may
relay a fully signed transaction.
