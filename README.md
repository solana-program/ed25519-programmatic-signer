# SPL Programmatic Signer

Sign a transaction offline with a cold Ed25519 authority, then submit it through a
programmatic signer PDA using an online fee payer. Three programs provide nonce
storage, signature verification, and legacy-message execution.

Start with the [manual quickstart](docs/quickstart.md) to build the CLI, prepare a
transfer, inspect and sign it offline, and relay it on Devnet. The programs are
already deployed. The walkthrough also covers token transfers, multiple signatures,
relayer restrictions, and nonce chains.

Read [how it works](docs/architecture.md) for the program execution path.

For development, `make check-clients` runs formatting, lint, build, and client tests.
