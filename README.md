# SPL Programmatic Signer

Sign a transaction offline with a cold Ed25519 authority, then relay it through a
programmatic signer PDA with an online fee payer. The stack combines three programs:
nonce storage, Ed25519 signature verification, and legacy-message execution.

This branch restores client workflows against the current interfaces. Start with a
local validator: current source program IDs have no verified public deployment.
Historical transaction files and deployment addresses are incompatible with this stack.

```sh
make check-clients
make demo-local
```

The demo creates disposable local keys, starts its own validator, and exercises SOL
transfers, a precomputed nonce chain, cancellation, and a local SPL Token mint. It
stops only its own validator and retains artifacts under `target/local-demo/`.

- [Quickstart](docs/quickstart.md): prerequisites and a complete offline signing flow.
- [Clients](docs/clients.md): CLI commands, transaction files, Rust APIs, and examples.
- [Architecture](docs/architecture.md): authorities, signatures, nonce transitions, and limits.
- [Migration](docs/migration.md): differences from the historical `clients-mvp` branch.
- [Local validation](docs/local-validation.md): completed checks and review findings.
- [Deployment status](docs/deployment-status.md): identities and work pending before devnet.

The per-program Rust clients and interfaces remain the low-level building blocks.
`clients/rust` provides RPC-free transaction workflows; `clients/cli` supplies standard
Solana configuration, signer loading, RPC submission, and offline file commands.
`clients/js` remains generated from the current interfaces.
