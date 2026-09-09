# Migrating from clients-mvp

This is a selective port onto the current program interfaces. Historical
`clients-mvp` files and public deployments must not be assumed compatible.

| Historical surface | Current branch |
| --- | --- |
| `psigner` binary | `spl-programmatic-signer-cli` |
| `spl-programmatic-signer-rust` | `spl-programmatic-signer-client` |
| Custom RPC, config, and wallet plumbing | Existing standard Solana CLI integration |
| Duplicated nonce account helpers | `spl-nonce-client` and `spl-nonce-interface` |
| Older executor variants and account lists | Current legacy-message executor, two fixed accounts |
| Historical program IDs | Current IDs in [deployment status](deployment-status.md) |
| Slot-dependent nonce assumptions | Exact inner-message transition and conditional nonce chains |

Rebuild inner messages and transaction files with the current crates and IDs.
Validate nonce account ownership and the strict current state layout. An upgrade
can change program behavior, permitted executors, signing semantics, or nonce
transitions; continued validity of pre-signed files is not guaranteed across upgrades.
Choose deployment identity and asset migration explicitly before using public funds.
Changing a signer program ID also changes its derived PDA addresses.

The restored CLI covers creation, inspection, signing and batch signing, merging,
verification, simulation, relay submission, and cancellation. It adds explicit
successor prediction and predecessor-based construction for the current nonce
semantics. V0/address lookup table support is not part of this port.

`nonce create` preserves the existing optional generated keypair behavior. Its
`--cold-authority` derives a PDA; `--nonce-authority` stores a literal address. A
keypair-authorized nonce remains a low-level use case; CLI cancellation uses the
cold-authority PDA path. The nonce decoder returns `Option<Nonce>` and rejects
uninitialized or incorrectly sized data.

Review changes in local branch `clients-mvp-v2`. The original `clients-mvp` and
`rust-client-prepare` branch references were preserved. No public deployment or
package publication is part of this migration.
