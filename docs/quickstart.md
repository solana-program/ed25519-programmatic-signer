# Quickstart

This walkthrough teaches you to prepare a SOL transfer, inspect and sign it offline,
then submit it with an online fee payer. Run steps 1–7 in order. After that, choose
from the optional recipes for tokens, multiple signatures, designated relayers,
nonce chains, cancellation, and other program instructions.

All transactions here use **Devnet** and its test SOL. The three programs are
already deployed; you only need to build the CLI:

| Program | Devnet address |
| --- | --- |
| Nonce | [Noncediea1fH12usShuQAz28UhgAeuE5Maf32LsMUQB](https://explorer.solana.com/address/Noncediea1fH12usShuQAz28UhgAeuE5Maf32LsMUQB?cluster=devnet) |
| Signer | [EdSigVfK1DkeMrjFNDMjwfQaJPhPTtX7jW8uPv3oKEgN](https://explorer.solana.com/address/EdSigVfK1DkeMrjFNDMjwfQaJPhPTtX7jW8uPv3oKEgN?cluster=devnet) |
| Executor | [ExecxgyHYsAXB4c5dZodV1zJZ9hqfsDCYkRDRATrpkFR](https://explorer.solana.com/address/ExecxgyHYsAXB4c5dZodV1zJZ9hqfsDCYkRDRATrpkFR?cluster=devnet) |

## Before you start

Use Bash in one terminal opened at the repository root. You need:

- Rust from `rust-toolchain.toml` and `nightly-2026-01-22` for the CLI build. If the
  nightly is missing, install it with `rustup toolchain install nightly-2026-01-22`.
- Solana CLI **3.1.8**, including `solana-keygen`.
- `make` and `jq`. The token recipe additionally needs SPL Token CLI **5.5.0**.

Three roles participate. You will play all three on one machine using disposable
keys, while keeping their responsibilities distinct:

| Role | What it does | Key or account |
| --- | --- | --- |
| Coordinator | Builds a transaction file from instructions and the current nonce | Only needs the cold authority's public address |
| Cold signer | Reviews and signs the file; needs no RPC or SOL | The cold authority key |
| Relayer | Submits the signed file and pays the network fee | A funded online key |

The **programmatic signer PDA** is the address that owns and spends the assets.
It is derived from the cold authority, but has no private key. The **SPL Nonce
account** holds a value that changes when a transaction succeeds, preventing replay.
See [how it works](architecture.md) for the program execution path.

## 1. Build the CLI

Build the command-line client from this checkout:

```sh
make build-clients-cli
```

## 2. Create keys and select Devnet

Set `PCLI` to the programmatic signer CLI, then create a working directory and
three local keypair files:

```sh
PCLI="$PWD/target/debug/spl-programmatic-signer-cli"
WORK=$(mktemp -d "$PWD/target/quickstart.XXXXXX")
RPC=https://api.devnet.solana.com
for key in payer cold recipient; do
  solana-keygen new \
    --silent \
    --no-bip39-passphrase \
    --outfile "$WORK/$key.json"
done
PAYER="$WORK/payer.json"
COLD="$WORK/cold.json"
```

Online commands below specify Devnet with `--url "$RPC"`. Commands that sign
select the payer or authority with `--keypair`, `--fee-payer`, or `--owner`. These
flags apply to that invocation; your saved Solana CLI configuration stays unchanged.
Record the cluster and public addresses for this walkthrough:

```sh
solana cluster-version \
  --url "$RPC" \
  --commitment confirmed
GENESIS_HASH=$(
  solana genesis-hash \
    --url "$RPC" \
    --commitment confirmed
)
COLD_ADDRESS=$(solana address --keypair "$COLD")
RECIPIENT=$(solana-keygen pubkey "$WORK/recipient.json")
PDA=$("$PCLI" address "$COLD_ADDRESS")
printf 'Payer: %s\nCold authority: %s\nPDA: %s\nRecipient: %s\nGenesis hash: %s\n' \
  "$(solana-keygen pubkey "$PAYER")" "$COLD_ADDRESS" "$PDA" "$RECIPIENT" "$GENESIS_HASH"
```

The PDA should differ from the cold address. Open the
[Solana Devnet faucet](https://faucet.solana.com/), select Devnet, and paste the
**Payer** address printed above. Request at least **1 Devnet SOL**, enough for the
walkthrough and all optional recipes. Once it arrives, check the payer's balance:

```sh
solana balance "$(solana-keygen pubkey "$PAYER")" \
  --url "$RPC" \
  --commitment confirmed
```

Confirm the payer is funded before continuing. The cold authority needs no SOL.
For actual offline use, its key stays on the signing machine and only its public
address is given to the coordinator.

## 3. Create a nonce and fund the PDA

Create an SPL Nonce account controlled by the cold authority's PDA. The command
generates the nonce account's creation keypair automatically:

```sh
"$PCLI" nonce create \
  --url "$RPC" \
  --commitment confirmed \
  --fee-payer "$PAYER" \
  --output json-compact \
  --cold-authority "$COLD_ADDRESS" > "$WORK/nonce.json"
NONCE_ACCOUNT=$(jq -er .nonceAccount "$WORK/nonce.json")
NONCE_VALUE=$(jq -er .nonce "$WORK/nonce.json")
"$PCLI" nonce show "$NONCE_ACCOUNT" \
  --url "$RPC" \
  --commitment confirmed
solana transfer "$PDA" 0.1 \
  --url "$RPC" \
  --commitment confirmed \
  --keypair "$PAYER" \
  --allow-unfunded-recipient
solana balance "$PDA" \
  --url "$RPC" \
  --commitment confirmed
```

The nonce's authority should match `PDA`, and the PDA balance should be **0.1 SOL**.
These are the assets the inner transfer will spend. The online payer separately
pays for account creation and network fees.

## 4. Prepare the transfer without signing or submitting it

Use the stock Solana CLI to build a transfer from the PDA to the recipient:

```sh
solana transfer "$RECIPIENT" 0.001 \
  --url "$RPC" \
  --commitment confirmed \
  --from "$PDA" \
  --fee-payer "$PDA" \
  --blockhash "$NONCE_VALUE" \
  --sign-only \
  --dump-transaction-message \
  --output json-compact \
  --allow-unfunded-recipient > "$WORK/transfer.source.json"
jq '{blockhash, absent}' "$WORK/transfer.source.json"
```

The output should list the PDA as an absent signer and show `NONCE_VALUE` as the
blockhash. This is expected: a PDA cannot sign with a keypair. Here `--blockhash`
carries the SPL nonce value; do not use Solana's native `--nonce` option, which
uses a different protocol. `--fee-payer "$PDA"` identifies an inner signer; the
online payer will pay the actual network fee when relaying.

Wrap the source message into the transaction file that the cold authority signs:

```sh
"$PCLI" transaction create \
  --url "$RPC" \
  --commitment confirmed \
  --from-sign-only "$WORK/transfer.source.json" \
  --nonce "$NONCE_ACCOUNT" \
  --authority "$COLD_ADDRESS" \
  --fetch-nonce \
  --outfile "$WORK/transfer.json"
"$PCLI" transaction simulate inner "$WORK/transfer.json" \
  --url "$RPC" \
  --commitment confirmed \
  --fee-payer "$PAYER"
```

Expect `inner simulation succeeded`. This checks the transfer against current
accounts without consuming the nonce or requiring cold signatures. It is a preview;
state can still change before submission.

Keep the three transaction files distinct:

| File | Contents | Next user |
| --- | --- | --- |
| `transfer.source.json` | Instructions and nonce from the stock CLI's sign-only output | Coordinator |
| `transfer.json` | Wrapped transaction with empty signature slots | Cold signer |
| `transfer.signed.json` | The same wrapped message with signatures added in step 5 | Relayer |

These files contain no private keys. Wrapped files use JSON serialization of
`solana_transaction::Transaction`; signatures cover the binary message, so changing
JSON whitespace does not invalidate them. These wrapped files are inputs to
`transaction submit`, which builds the live network transaction.

## 5. Inspect, then sign offline

Run these commands in the same terminal for this exercise. With separate machines,
move `transfer.json` to the signing machine, adjust the file paths, and inspect it
there. `inspect` and `sign` do not query RPC. The signer is selected explicitly
with `--keypair "$COLD"`.

```sh
"$PCLI" transaction inspect "$WORK/transfer.json"
```

Before signing, check that inspection shows:

- A transfer of **1,000,000 lamports** from `PDA` to `RECIPIENT`.
- `NONCE_ACCOUNT` and `NONCE_VALUE` as the nonce account and expected nonce.
- The same genesis hash recorded in `GENESIS_HASH` during setup.
- `COLD_ADDRESS` as the transaction signer, currently marked missing.

The predicted next nonce is what this exact inner message will produce if it
succeeds. Signing changes only the signature slots:

```sh
"$PCLI" transaction sign "$WORK/transfer.json" \
  --keypair "$COLD" \
  --outfile "$WORK/transfer.signed.json"
"$PCLI" transaction inspect "$WORK/transfer.signed.json"
```

The authority should now be marked signed, with the same transfer and nonce.
Return only the signed transaction file to the relayer; keep the cold key offline.

## 6. Verify, simulate the relay, and submit

The online relayer checks the signed file against the live nonce and cluster:

```sh
"$PCLI" transaction verify "$WORK/transfer.signed.json" \
  --url "$RPC" \
  --commitment confirmed \
  --fetch-nonce
"$PCLI" transaction simulate relay "$WORK/transfer.signed.json" \
  --url "$RPC" \
  --commitment confirmed \
  --fee-payer "$PAYER"
```

Expect `Fully signed: true` and `relay simulation succeeded`. Relay simulation
checks the complete programmatic signing and nonce execution path without sending
it. Now submit with the online payer selected by `--fee-payer "$PAYER"`:

```sh
"$PCLI" transaction submit "$WORK/transfer.signed.json" \
  --url "$RPC" \
  --commitment confirmed \
  --fee-payer "$PAYER"
solana balance "$RECIPIENT" \
  --url "$RPC" \
  --commitment confirmed \
  --lamports
"$PCLI" nonce show "$NONCE_ACCOUNT" \
  --url "$RPC" \
  --commitment confirmed
```

Submission prints a confirmed signature and predicted/observed successor nonce.
The recipient should hold **1,000,000 lamports**. The stored nonce should have
changed to the predicted successor; the original file no longer matches it.

## 7. Check replay protection

Submit the same file again. **This command is expected to fail** with
`nonce mismatch`; continue after that error. A different error needs investigation.

```sh
"$PCLI" transaction submit "$WORK/transfer.signed.json" \
  --url "$RPC" \
  --commitment confirmed \
  --fee-payer "$PAYER"
```

The transfer cannot land twice. To send another transaction, build a new source
message using the current nonce, or prepare a chain as below.

## Optional recipes

These recipes reuse the same session's keys, Devnet URL, funded PDA, recipient,
and nonce account after step 7. Each reads the nonce it needs, so you may choose them
independently. Run each recipe once in this working directory; use new filenames
if you repeat one.

### Pre-sign a chain and sign a batch

Continue in the same session. Read the current nonce and prepare the first file:

```sh
NONCE_VALUE=$(
  "$PCLI" nonce show "$NONCE_ACCOUNT" \
    --url "$RPC" \
    --commitment confirmed \
    --output json-compact |
  jq -er .nonce
)
solana transfer "$RECIPIENT" 0.001 \
  --url "$RPC" \
  --commitment confirmed \
  --from "$PDA" \
  --fee-payer "$PDA" \
  --blockhash "$NONCE_VALUE" \
  --sign-only \
  --dump-transaction-message \
  --output json-compact > "$WORK/first.source.json"
"$PCLI" transaction create \
  --url "$RPC" \
  --commitment confirmed \
  --from-sign-only "$WORK/first.source.json" \
  --nonce "$NONCE_ACCOUNT" \
  --authority "$COLD_ADDRESS" \
  --fetch-nonce \
  --outfile "$WORK/first.json"
```

`inspect` includes `nextNonce`, conditional on that exact message succeeding.
Use it to build the second file before submitting the first. `--after` checks
the successor against its predecessor and supplies the genesis hash without RPC:

```sh
"$PCLI" transaction inspect "$WORK/first.json"
NEXT=$("$PCLI" transaction inspect "$WORK/first.json" --output json-compact | jq -er .nextNonce)
solana transfer "$RECIPIENT" 0.001 \
  --url "$RPC" \
  --commitment confirmed \
  --from "$PDA" \
  --fee-payer "$PDA" \
  --blockhash "$NEXT" \
  --sign-only \
  --dump-transaction-message \
  --output json-compact > "$WORK/second.source.json"
"$PCLI" transaction create \
  --from-sign-only "$WORK/second.source.json" \
  --nonce "$NONCE_ACCOUNT" \
  --authority "$COLD_ADDRESS" \
  --after "$WORK/first.json" \
  --outfile "$WORK/second.json"
"$PCLI" transaction inspect "$WORK/second.json"
mkdir "$WORK/signed"
"$PCLI" transaction sign "$WORK/first.json" "$WORK/second.json" \
  --keypair "$COLD" \
  --outdir "$WORK/signed"
```

Submit the first file, then the second. A competing transition would invalidate
the planned descendants. Use separate nonce accounts when transactions must
proceed independently.

```sh
"$PCLI" transaction submit "$WORK/signed/first.json" \
  --url "$RPC" \
  --commitment confirmed \
  --fee-payer "$PAYER"
"$PCLI" transaction submit "$WORK/signed/second.json" \
  --url "$RPC" \
  --commitment confirmed \
  --fee-payer "$PAYER"
```

### Cancel a pending file

Prepare and sign another transfer, but leave it unsubmitted:

```sh
NONCE_VALUE=$(
  "$PCLI" nonce show "$NONCE_ACCOUNT" \
    --url "$RPC" \
    --commitment confirmed \
    --output json-compact |
  jq -er .nonce
)
solana transfer "$RECIPIENT" 0.001 \
  --url "$RPC" \
  --commitment confirmed \
  --from "$PDA" \
  --fee-payer "$PDA" \
  --blockhash "$NONCE_VALUE" \
  --sign-only \
  --dump-transaction-message \
  --output json-compact > "$WORK/pending.source.json"
"$PCLI" transaction create \
  --url "$RPC" \
  --commitment confirmed \
  --from-sign-only "$WORK/pending.source.json" \
  --nonce "$NONCE_ACCOUNT" \
  --authority "$COLD_ADDRESS" \
  --fetch-nonce \
  --outfile "$WORK/pending.json"
"$PCLI" transaction inspect "$WORK/pending.json"
"$PCLI" transaction sign "$WORK/pending.json" \
  --keypair "$COLD" \
  --outfile "$WORK/pending.signed.json"
```

Build an empty transaction that consumes the same nonce. Inspection should show no
inner instructions and the same expected nonce as the pending file. Sign and submit:

```sh
"$PCLI" nonce advance \
  --from-transaction "$WORK/pending.json" \
  --authority "$COLD_ADDRESS" \
  --outfile "$WORK/cancel.json"
"$PCLI" transaction inspect "$WORK/cancel.json"
"$PCLI" transaction sign "$WORK/cancel.json" \
  --keypair "$COLD" \
  --outfile "$WORK/cancel.signed.json"
"$PCLI" transaction submit "$WORK/cancel.signed.json" \
  --url "$RPC" \
  --commitment confirmed \
  --fee-payer "$PAYER"
```

Confirmed cancellation invalidates `pending.signed.json`. If the pending transfer
lands first, cancellation fails instead.

### Collect multiple cold signatures

Add a second approval authority. Every listed authority must sign this file; the
transfer still spends from the first authority's PDA. The nonce account's authority
remains unchanged:

```sh
solana-keygen new \
  --silent \
  --no-bip39-passphrase \
  --outfile "$WORK/cold-2.json"
COLD_ADDRESS_2=$(solana-keygen pubkey "$WORK/cold-2.json")
NONCE_VALUE=$(
  "$PCLI" nonce show "$NONCE_ACCOUNT" \
    --url "$RPC" \
    --commitment confirmed \
    --output json-compact |
  jq -er .nonce
)
solana transfer "$RECIPIENT" 0.001 \
  --url "$RPC" \
  --commitment confirmed \
  --from "$PDA" \
  --fee-payer "$PDA" \
  --blockhash "$NONCE_VALUE" \
  --sign-only \
  --dump-transaction-message \
  --output json-compact > "$WORK/multisig.source.json"
"$PCLI" transaction create \
  --url "$RPC" \
  --commitment confirmed \
  --from-sign-only "$WORK/multisig.source.json" \
  --nonce "$NONCE_ACCOUNT" \
  --authority "$COLD_ADDRESS" \
  --authority "$COLD_ADDRESS_2" \
  --fetch-nonce \
  --outfile "$WORK/multisig.json"
"$PCLI" transaction inspect "$WORK/multisig.json"
"$PCLI" transaction sign "$WORK/multisig.json" \
  --keypair "$COLD" \
  --outfile "$WORK/multisig.first.json"
"$PCLI" transaction sign "$WORK/multisig.json" \
  --keypair "$WORK/cold-2.json" \
  --outfile "$WORK/multisig.second.json"
"$PCLI" transaction merge "$WORK/multisig.first.json" "$WORK/multisig.second.json" --outfile "$WORK/multisig.signed.json"
"$PCLI" transaction verify "$WORK/multisig.signed.json" \
  --url "$RPC" \
  --commitment confirmed \
  --fetch-nonce
"$PCLI" transaction simulate relay "$WORK/multisig.signed.json" \
  --url "$RPC" \
  --commitment confirmed \
  --fee-payer "$PAYER"
"$PCLI" transaction submit "$WORK/multisig.signed.json" \
  --url "$RPC" \
  --commitment confirmed \
  --fee-payer "$PAYER"
```

Each authority independently inspects and signs a copy. Merge accepts only copies
of exactly the same message with valid signatures. Verification should report
`Fully signed: true` after merging both approvals.

### Require a designated relayer

Ordinarily anyone holding a fully signed file may relay it. To restrict that,
make a separate relayer key an inner signer and name it with `--submit-signer`.
Here it is the inner fee-payer slot; the funded online payer still pays network fees:

```sh
solana-keygen new \
  --silent \
  --no-bip39-passphrase \
  --outfile "$WORK/relayer.json"
RELAYER=$(solana-keygen pubkey "$WORK/relayer.json")
NONCE_VALUE=$(
  "$PCLI" nonce show "$NONCE_ACCOUNT" \
    --url "$RPC" \
    --commitment confirmed \
    --output json-compact |
  jq -er .nonce
)
solana transfer "$RECIPIENT" 0.001 \
  --url "$RPC" \
  --commitment confirmed \
  --from "$PDA" \
  --fee-payer "$RELAYER" \
  --blockhash "$NONCE_VALUE" \
  --sign-only \
  --dump-transaction-message \
  --output json-compact > "$WORK/designated.source.json"
"$PCLI" transaction create \
  --url "$RPC" \
  --commitment confirmed \
  --from-sign-only "$WORK/designated.source.json" \
  --nonce "$NONCE_ACCOUNT" \
  --authority "$COLD_ADDRESS" \
  --submit-signer "$RELAYER" \
  --fetch-nonce \
  --outfile "$WORK/designated.json"
"$PCLI" transaction inspect "$WORK/designated.json"
"$PCLI" transaction sign "$WORK/designated.json" \
  --keypair "$COLD" \
  --outfile "$WORK/designated.cold.json"
"$PCLI" transaction sign "$WORK/designated.cold.json" \
  --keypair "$WORK/relayer.json" \
  --outfile "$WORK/designated.ready.json"
```

Without the live relayer key, simulation is expected to fail with `missing outer
signer` even though both file signatures are present:

```sh
"$PCLI" transaction simulate relay "$WORK/designated.ready.json" \
  --url "$RPC" \
  --commitment confirmed \
  --fee-payer "$PAYER"
```

Supply that key to simulate and submit successfully:

```sh
"$PCLI" transaction simulate relay "$WORK/designated.ready.json" \
  --url "$RPC" \
  --commitment confirmed \
  --fee-payer "$PAYER" \
  --submit-signer "$WORK/relayer.json"
"$PCLI" transaction submit "$WORK/designated.ready.json" \
  --url "$RPC" \
  --commitment confirmed \
  --fee-payer "$PAYER" \
  --submit-signer "$WORK/relayer.json"
```

The relayer signs both the file and the live outer transaction. A different fee
payer cannot bypass the required relayer's live signature.

### Transfer SPL tokens

Create a six-decimal test mint and two token accounts on Devnet, then mint two
tokens to the PDA. These setup transactions use the online payer:

```sh
for key in mint source-token destination-token; do
  solana-keygen new \
    --silent \
    --no-bip39-passphrase \
    --outfile "$WORK/$key.json"
done
MINT=$(solana-keygen pubkey "$WORK/mint.json")
SOURCE_TOKEN=$(solana-keygen pubkey "$WORK/source-token.json")
DESTINATION_TOKEN=$(solana-keygen pubkey "$WORK/destination-token.json")
spl-token create-token "$WORK/mint.json" \
  -u "$RPC" \
  --decimals 6 \
  --fee-payer "$PAYER" \
  --mint-authority "$PAYER"
spl-token create-account "$MINT" "$WORK/source-token.json" \
  -u "$RPC" \
  --owner "$PDA" \
  --fee-payer "$PAYER"
spl-token create-account "$MINT" "$WORK/destination-token.json" \
  -u "$RPC" \
  --owner "$RECIPIENT" \
  --fee-payer "$PAYER"
spl-token mint "$MINT" 2 "$SOURCE_TOKEN" \
  -u "$RPC" \
  --fee-payer "$PAYER" \
  --mint-authority "$PAYER"
```

Build a checked transfer of 1.25 tokens, inspect the mint, amount and decimals,
then follow the same signing and relay flow:

```sh
NONCE_VALUE=$(
  "$PCLI" nonce show "$NONCE_ACCOUNT" \
    --url "$RPC" \
    --commitment confirmed \
    --output json-compact |
  jq -er .nonce
)
spl-token transfer "$MINT" 1.25 "$DESTINATION_TOKEN" \
  -u "$RPC" \
  --from "$SOURCE_TOKEN" \
  --owner "$PDA" \
  --fee-payer "$PDA" \
  --blockhash "$NONCE_VALUE" \
  --mint-decimals 6 \
  --sign-only \
  --dump-transaction-message \
  --output json-compact > "$WORK/token.source.json"
"$PCLI" transaction create \
  --url "$RPC" \
  --commitment confirmed \
  --from-sign-only "$WORK/token.source.json" \
  --nonce "$NONCE_ACCOUNT" \
  --authority "$COLD_ADDRESS" \
  --fetch-nonce \
  --outfile "$WORK/token.json"
"$PCLI" transaction simulate inner "$WORK/token.json" \
  --url "$RPC" \
  --commitment confirmed \
  --fee-payer "$PAYER"
"$PCLI" transaction inspect "$WORK/token.json"
"$PCLI" transaction sign "$WORK/token.json" \
  --keypair "$COLD" \
  --outfile "$WORK/token.signed.json"
"$PCLI" transaction verify "$WORK/token.signed.json" \
  --url "$RPC" \
  --commitment confirmed \
  --fetch-nonce
"$PCLI" transaction simulate relay "$WORK/token.signed.json" \
  --url "$RPC" \
  --commitment confirmed \
  --fee-payer "$PAYER"
"$PCLI" transaction submit "$WORK/token.signed.json" \
  --url "$RPC" \
  --commitment confirmed \
  --fee-payer "$PAYER"
spl-token balance \
  -u "$RPC" \
  --address "$SOURCE_TOKEN"
spl-token balance \
  -u "$RPC" \
  --address "$DESTINATION_TOKEN"
```

The source holds 0.75 tokens and the recipient holds 1.25. A token-owning PDA
need not hold SOL; the online payer covers account creation and relay fees.

### Include other program instructions

The import path accepts a legacy message containing instructions for other programs.
As a runnable example, use the stock CLI to add a Memo instruction to a SOL transfer:

```sh
NONCE_VALUE=$(
  "$PCLI" nonce show "$NONCE_ACCOUNT" \
    --url "$RPC" \
    --commitment confirmed \
    --output json-compact |
  jq -er .nonce
)
solana transfer "$RECIPIENT" 0.001 \
  --url "$RPC" \
  --commitment confirmed \
  --from "$PDA" \
  --fee-payer "$PDA" \
  --blockhash "$NONCE_VALUE" \
  --with-memo 'CLI walkthrough memo' \
  --sign-only \
  --dump-transaction-message \
  --output json-compact > "$WORK/memo.source.json"
"$PCLI" transaction create \
  --url "$RPC" \
  --commitment confirmed \
  --from-sign-only "$WORK/memo.source.json" \
  --nonce "$NONCE_ACCOUNT" \
  --authority "$COLD_ADDRESS" \
  --fetch-nonce \
  --outfile "$WORK/memo.json"
"$PCLI" transaction inspect "$WORK/memo.json"
"$PCLI" transaction simulate inner "$WORK/memo.json" \
  --url "$RPC" \
  --commitment confirmed \
  --fee-payer "$PAYER"
```

Inspection should show both the SOL transfer and the memo text. Both instructions
belong to the same signed message and execute in the same transaction:

```sh
"$PCLI" transaction sign "$WORK/memo.json" \
  --keypair "$COLD" \
  --outfile "$WORK/memo.signed.json"
"$PCLI" transaction verify "$WORK/memo.signed.json" \
  --url "$RPC" \
  --commitment confirmed \
  --fetch-nonce
"$PCLI" transaction simulate relay "$WORK/memo.signed.json" \
  --url "$RPC" \
  --commitment confirmed \
  --fee-payer "$PAYER"
"$PCLI" transaction submit "$WORK/memo.signed.json" \
  --url "$RPC" \
  --commitment confirmed \
  --fee-payer "$PAYER"
```

For a custom program without a sign-only CLI, construct its instructions with the
Solana SDK. Use the PDA wherever your program requires its signing authority, set
the legacy message's blockhash to the current SPL nonce, and export this source
JSON shape (the values below are placeholders):

```json
{
  "blockhash": "NONCE_VALUE",
  "message": "BASE64_SERIALIZED_LEGACY_MESSAGE"
}
```

Then use the same `transaction create --from-sign-only` command. A source message
is not yet a wrapped transaction file. Unknown instructions display raw program,
account, and data fields during inspection; reviewing them requires knowledge of
the target program. See [the Rust client pointers](architecture.md) for integration.

## Hardware-wallet signer sources

The CLI accepts Solana signer URLs, including `usb://ledger?key=0`. Physical device
signing is not yet verified. To try it in a fresh walkthrough, replace the `COLD`
assignment in step 2 with the following **before deriving `COLD_ADDRESS` and `PDA`**:

```sh
COLD='usb://ledger?key=0'
```

Continue step 2 and create the nonce and fund the resulting PDA in step 3. The
existing `--keypair "$COLD"` commands will use that signer source. Keep the URL
quoted. An existing file prepared for a different authority cannot be signed by
substituting a different key. Signer URLs also work with `--fee-payer` and
`--submit-signer`.

## Command reference

The CLI accepts `--url`, `--fee-payer`, `--commitment`, and Solana signer sources.
Runtime flags take precedence over saved configuration. Use each command's
`--help` for flags. Summaries support `--output json` and `--output json-compact`.
Transaction readers accept `-` for stdin; writers use `--outfile` or stdout.

Every command above serves a documented flow:

| Commands | Purpose | RPC |
| --- | --- | --- |
| `address` | Derive the asset-owning PDA | No |
| `nonce create`, `nonce show` | Initialize and refresh nonce state | Yes |
| `nonce advance` | Build a cancellation | Only with `--nonce`; `--from-transaction` is offline |
| `transaction create` | Wrap a source message | With `--fetch-nonce`; explicit snapshots and `--after` are offline |
| `transaction inspect`, `sign`, `merge` | Review and collect signatures; inspect also predicts the successor | No |
| `transaction verify` | Check signatures, cluster and nonce before relay | With `--fetch-nonce`; explicit snapshots are offline |
| `transaction simulate inner`, `simulate relay` | Rehearse before signing or submitting | Yes |
| `transaction submit` | Verify, relay, and confirm | Yes |

Offline creation accepts `--nonce-value` and `--genesis-hash`; verification also
requires `--nonce-authority`. These snapshots do not establish current chain state.
Submission checks the live nonce, cluster genesis hash, and all signatures again.
