# SPL Programmatic Signer CLI

```sh
make build-clients-cli
target/debug/spl-programmatic-signer-cli --help
make check-clients
make demo-local
```

Use the [local quickstart](../../docs/quickstart.md) and
[command reference](../../docs/clients.md). The executable uses standard Solana
configuration and signer sources. Offline file operations require no RPC; live
verification and submission explicitly check the current nonce and cluster.
