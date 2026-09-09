# Local validation — September 9, 2026

All checks below passed on `clients-mvp-v2` using local builds and a disposable local
validator. No GitHub Actions run, push, public deployment, or package publication was
started. The protected local branch references remain:

- `rust-client-prepare`: `c3cb8ece70ac9eee44ae9176ecf8fe072dfd1327`
- `clients-mvp`: `1897cdcaa5b00821834faafb2a94fe2a5fc47662`

## Results

| Check | Result |
| --- | --- |
| `make check-clients` | Formatting, strict Clippy, docs, feature powersets, SBF builds, and 57 Rust/CLI tests passed |
| Remaining nine Rust packages | Formatting, strict Clippy, docs, feature powersets, and 118 tests passed |
| CI no-std matrix | Both core targets and all seven alloc targets passed |
| `make generate-clients` | IDL and JavaScript regeneration succeeded with no generated-file changes |
| JavaScript formatting, lint, build, tests | 12 tests passed |
| Both Rust examples | Executed successfully without RPC |
| `make demo-local` | Actual Solana and SPL Token CLI inputs completed the full flow |
| `make spellcheck` and ShellCheck | Passed |
| `make audit` | Passed with 15 allowed dependency warnings under the existing policy |
| `git diff --check` | Passed |

The audit warnings concern existing dependencies. Comparing lockfiles against the
starting branch found no added, removed, or version-changed external packages.
No advisory suppressions were added. Local test dependencies still include the
existing Solana validator stack.

The client tests include offline commands against an unreachable RPC URL, partial
signatures and merging, rejection of invalid signatures and overwritten outputs,
cluster mismatches, packet size, SOL transfer execution, conditional nonce chains,
designated live relayers, cancellation, replay rejection, and rollback on inner
failure. The token test gives its PDA owner zero SOL and exercises inner simulation,
full relay simulation, actual submission, and resulting token balances.

The shell demo used Solana CLI 3.1.8 and SPL Token CLI 5.5.0. It imported their real
sign-only JSON, submitted a pre-signed SOL chain, rejected early/outdated submissions,
cancelled a pending file, and transferred 1.25 tokens from a fresh six-decimal mint.
The standalone validator ran only on loopback and was stopped by the demo's cleanup.
Hardware-wallet behavior and public-cluster deployments remain unverified.

## Review feedback

The first new commit, `61e8154`, addresses the remaining [PR 17 review
feedback](https://github.com/solana-program/ed25519-programmatic-signer/pull/17):
unpin the validator dependency, return an optional nonce decode result, strengthen
nonce creation assertions, use the package name as the binary name, share output
structs with tests, and remove redundant account polling after confirmation. It
preserves the two existing local changes for generated nonce keypairs and removing
the redundant rent output field.

A subsequent standards and spec review identified inner simulation's fee payer and
loss of confirmed-signature context on a failed follow-up nonce read. Both were
fixed. The follow-up review found no remaining actionable issues. Shared validated
wrapper decoding and typed command outputs were also incorporated.

## Repeat the checks

Start with `make check-clients` and `make demo-local`. The remaining Rust packages
use the same existing Makefile pattern targets:

```sh
for package in executor-client executor-interface executor-program \
  nonce-client nonce-interface nonce-program signer-client signer-interface signer-program; do
  make "format-check-$package" "clippy-$package" "build-doc-$package" \
    "powerset-$package" "test-$package" || exit 1
done
make check-no-std-core-nonce-interface check-no-std-core-nonce-program
make check-no-std-alloc-executor-client check-no-std-alloc-executor-interface \
  check-no-std-alloc-executor-program check-no-std-alloc-nonce-client \
  check-no-std-alloc-signer-client check-no-std-alloc-signer-interface \
  check-no-std-alloc-signer-program
make generate-clients
make format-check-js-clients-js lint-js-clients-js test-js-clients-js
make spellcheck audit
shellcheck scripts/demo-local.sh
```
