# Deployment status

Read-only RPC observations on September 9, 2026 found no accounts at the current
source IDs on devnet or mainnet-beta. Testnet RPC was unavailable during that check,
so its state is unverified. These observations are dated, not ongoing monitoring.

| Program | Current source ID |
| --- | --- |
| Nonce | `Noncediea1fH12usShuQAz28UhgAeuE5Maf32LsMUQB` |
| Ed25519 signer | `EdSigVfK1DkeMrjFNDMjwfQaJPhPTtX7jW8uPv3oKEgN` |
| Legacy executor | `ExecxgyHYsAXB4c5dZodV1zJZ9hqfsDCYkRDRATrpkFR` |

The historical devnet IDs below were present, with last deployment block times on
July 9, 2026. They were absent on mainnet-beta in the same check. Their upgrade
authority was `CgnDbeBKoNro2kfhkyWwmsFth7DFHfHDttUay1azbGg4`.

- Nonce: `Hr4SV37wbyBMCvDq9hbMU3qKebicuPmSz6AKdTd7ysrD`
- Signer: `54JfXE4CGxgRsFJkSupJ4kYWFbYauf2Us9GC4FUGCGmS`
- Executor: `3LqtPnGXhqYkXNwoHtWM68t1hUfPrVuAdyxN6CCpUZof`

Those deployments predate the current wire format and do not validate current
client compatibility. The local demo loads the current source IDs into a fresh
genesis without using deployment keys.

## Before a public rehearsal

Resolve custody of the vanity-ID keypairs or choose a coordinated replacement
identity. The generated keypairs under the existing local `target/deploy` directory
do not match the declared source IDs. They cannot deploy those identities.

After local validation and explicit authorization to deploy, select the program
IDs and upgrade authority, rebuild and verify all three artifacts, verify client
and executor policy IDs, and record the cluster genesis hash and deployed program
data. Then repeat the SOL, SPL Token, replay, chain, cancellation, and failure tests
on devnet with disposable funds. Review any migration of assets or authorities as a
separate operation. No pushes, deployments, or publications were performed for this
branch preparation.
