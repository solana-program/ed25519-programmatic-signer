# psigner CLI

This folder contains the `psigner` command-line client.

The CLI should contain CLI-only concerns: argument parsing, config loading, keypair and
remote-wallet handling, file IO, RPC submission, and user-facing output. Shared Solana
message/artifact helpers live in `clients/rust`.

The current MVP implements the file-first transaction flow:

```text
# Derive the ProgrammaticSigner PDA for a cold authority.
psigner address <AUTHORITY>

# Wrap upstream Solana/SPL Token sign-only JSON into a psigner artifact.
psigner transaction create --from-sign-only inner.json --nonce <NONCE_ACCOUNT> --authority <AUTHORITY> --fetch-nonce --outfile tx.psigner

# Decode the artifact for offline review before signing.
psigner transaction inspect tx.psigner

# Add a cold-authority signature from a local keypair or hardware-wallet URL.
psigner transaction sign tx.psigner --keypair authority.json --outfile tx.signed.psigner

# Merge signatures collected on separate copies of the same artifact.
psigner transaction merge tx.signed.part1.psigner tx.signed.part2.psigner --outfile tx.merged.psigner

# Verify artifact structure, signatures, and the live nonce account.
psigner transaction verify tx.signed.psigner --fetch-nonce

# Submit the fully signed artifact with the online relayer fee payer.
psigner transaction submit tx.signed.psigner --fee-payer relayer.json

# Create and initialize a nonce account controlled by a cold authority PDA.
psigner nonce create --programmatic-authority <COLD_AUTHORITY> --nonce-keypair nonce.json --fee-payer payer.json

# Show the current nonce account state.
psigner nonce show <NONCE_ACCOUNT>

# Build a cancellation artifact that advances the nonce used by an artifact.
psigner nonce advance --from-transaction tx.psigner --authority <COLD_AUTHORITY> --outfile advance.psigner
```

`transaction create`, `transaction verify`, `transaction submit`, and explicit
`nonce advance` creation fetch the cluster genesis hash from RPC by default when they
are already online. Use `--genesis-hash <HASH>` for fully offline creation or
verification from a known cluster snapshot.

Signer arguments that produce signatures (`--keypair`, `--fee-payer`, and
`--submit-signer`) accept local Solana keypair files and Solana remote-wallet URLs
such as `'usb://ledger?key=0'`. Quote hardware-wallet URLs in shells like zsh.
`--nonce-keypair` stays file-only because it creates the new nonce account address.
Inspect transaction files before hardware signing; Ledger may require blind signing for
this wrapped transaction format.
