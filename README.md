# SPL Programmatic Signer

Sign a transaction offline with a cold Ed25519 authority, then submit it through a
programmatic signer PDA using an online fee payer. Three programs provide nonce
storage, signature verification, and legacy-message execution.

```sh
make check-clients
make demo-local
```

The demo starts a disposable local validator and exercises SOL and SPL Token
transfers, offline signing, nonce chains, replay rejection, and cancellation.
It retains artifacts under `target/local-demo/` and stops its validator on exit.

- [Quickstart](docs/quickstart.md)
- [How it works](docs/architecture.md)
