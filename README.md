# Ed25519 Programmatic Signer

Long-lived authorizations for Solana. Sign now, submit later.

Solana's blockhash expiration window can make offline signing and collecting multiple approvals difficult. Programmatic
Signer lets authorities sign long-lived messages off-chain, which a relayer can submit later. The programs verify the
signatures, advance a nonce to prevent replays, and invoke the approved instructions via CPI with signer privilege
granted to PDAs derived from the authority's keys. These PDAs hold funds or serve as authorities for token, stake, and
other accounts.

> [!WARNING]
> Deployed on Devnet only and not yet fully audited. The crates on crates.io are name placeholders and the npm package
> is not yet published.

- **Ed25519 Signer:** `EdSigVfK1DkeMrjFNDMjwfQaJPhPTtX7jW8uPv3oKEgN`
- **Message Executor:** `ExecxgyHYsAXB4c5dZodV1zJZ9hqfsDCYkRDRATrpkFR`
- **Nonce:** `Noncediea1fH12usShuQAz28UhgAeuE5Maf32LsMUQB`

## Documentation

See the [Programmatic Signer documentation](https://www.solana-program.com/docs/programmatic-signer) for setup, usage,
migration, and client guides.
