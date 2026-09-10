# How it works

The cold authority is an Ed25519 key; its programmatic signer is a PDA derived by
the signer program. Assets and application authorities belong to the PDA. Signing
a wrapper authorizes that PDA's use without exposing the cold key to the relayer.

1. The **inner legacy message** contains the business instructions, required PDA/live
   signers, and the current SPL nonce in its blockhash field.
2. The **wrapper** contains one legacy-executor instruction and is signed by the cold
   authorities and any designated submit signers. Its blockhash contains the cluster
   genesis hash.
3. The **relay** uses an online fee payer and fresh network blockhash. The signer
   program verifies the wrapper signatures and invokes the executor with derived PDA
   signer privileges. The executor advances the nonce and runs the inner instructions
   atomically; failure rolls both back.

Only legacy messages are supported. The executor takes the nonce account and nonce
program before the inner accounts. Slot hashes are needed for nonce initialization.
Nonce accounts have a strict 64-byte layout containing the nonce and authority.
`nonce create --cold-authority` derives a PDA; `--nonce-authority` stores a literal address.

The successor commits to the nonce program, nonce account, current nonce, and exact
inner message. This permits pre-signed chains, conditional on each predecessor
succeeding. Cancellation consumes the same nonce with an empty inner message.

The CLI validates signatures, account privileges, program IDs, nonce state, and
cluster genesis. Genesis comparison is a client policy: the on-chain program does
not independently fetch and compare the cluster genesis hash. Custom relayers must
apply that check. Relays must fit the 1232-byte network packet and runtime limits.
Changing program IDs changes signer PDAs; program upgrades may invalidate pre-signed
files. A PDA does not become a native keypair account or wallet signer.

For Rust integrations, use the existing per-program clients: `executor/client`
builds `execute`; `signer/client` builds `wrapped_message` and `submit`;
`nonce/client` creates and decodes nonce accounts. Compose ordinary SDK messages
and set the inner blockhash to the nonce and the wrapper blockhash to the cluster
genesis hash before signing.
