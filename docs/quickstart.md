# Quickstart

A quick guide of the pre-release programmatic signer stack on devnet.
These programs are unaudited and will change. APIs and program IDs are not
stable. The devnet deployment exists for exploration only.

> **Deployment status:** the July 2026 devnet deployment documented below uses
> the pre-rebase program IDs and wire format. This branch now follows the newer
> execution-program design and is not compatible with that deployment. Do not
> submit artifacts built from this checkout to those historical programs. The
> procedural guide becomes runnable again after the current binaries are deployed
> and this section is updated.

## How it works

Every submission runs through three programs:

- `SPL Ed25519 Signer` verifies the authority signatures over the cold-signed
  transaction and promotes each authority's `ProgrammaticSigner` PDA to signer
  privilege.
- `SPL Message Executor` replays the inner Solana message instruction by
  instruction via CPI.
- `SPL Nonce` stores a one-time nonce per account. The executor consumes it
  before replay, so no transaction file can land twice; rollback restores it if
  replay fails.

```text
hot relay transaction — built by the relayer at submit time, pays the fee
  -> SPL Ed25519 Signer
      verifies the signatures on the cold-signed transaction
      promotes ProgrammaticSigner PDA signer privilege
      -> SPL Message Executor
          -> SPL Nonce
              advances the nonce before replay
          replays the inner message's instructions
```

Important terms to know:

- **hot relay transaction** — the live transaction the relayer builds when
  running `transaction submit`. It pays the fee and calls the signer program.
- **cold-signed transaction** — the standard `VersionedTransaction` that cold
  authorities sign. It may be unsigned, partially signed, or fully signed as it
  moves through the flow. The base64 serialized format of this that the `transaction`
  command reads/writes we call **transaction file**.
- **inner message** — the `VersionedMessage` the executor replays. It carries
  the actual instructions and is never signed. Its signer is a PDA that gains
  privilege through promotion.

How the pieces fit together in depth lives in [architecture.md](architecture.md).

## The three roles

- **Coordinator** (online) — reads the nonce account, builds the inner message
  with the stock Solana or SPL Token CLI, wraps it into a transaction file.
- **Cold signer** (air-gapped) — inspects the decoded file, signs it, re-emits
  it. Needs no RPC and no SOL.
- **Relayer** (online, pays) — verifies and rehearses the signed file, builds
  the hot relay transaction, submits it, and confirms.

One person can play all three, as this quickstart does from a single terminal.
In production the roles can sit on three machines with an air gap in the middle,
and the transaction file is the only thing that crosses it.

## Which commands need the network

The commands split cleanly by role. Everything the cold signer runs works with
no RPC and no SOL.

| Command                      | Network | Typical role |
|------------------------------|---------|--------------|
| `nonce create`, `nonce show` | yes     | coordinator  |
| `transaction create`         | no      | coordinator  |
| `transaction simulate inner` | yes     | coordinator  |
| `transaction inspect`        | no      | cold signer  |
| `transaction sign`           | no      | cold signer  |
| `transaction merge`          | no      | coordinator  |
| `transaction verify`         | no      | relayer      |
| `transaction simulate relay` | yes     | relayer      |
| `transaction submit`         | yes     | relayer      |
| `nonce advance` (build)      | no      | coordinator  |

Only commands that send a transaction need a funded fee payer.
This quickstart plays all three roles from one machine.

## Deployment IDs

The current checkout declares these execution-program IDs. As of August 13,
2026, none of them is deployed on devnet:

```text
SPL Nonce:              Noncediea1fH12usShuQAz28UhgAeuE5Maf32LsMUQB
SPL Ed25519 Signer:     EdSigVfK1DkeMrjFNDMjwfQaJPhPTtX7jW8uPv3oKEgN
SPL Message Executor:   ExecxgyHYsAXB4c5dZodV1zJZ9hqfsDCYkRDRATrpkFR
```

The historical July 9, 2026 deployment is still present on devnet under these
IDs, but it is not compatible with this checkout:

Created on July 9, 2026. Program IDs will not be the same when released
officially.

```text
Cluster:                devnet
Genesis hash:           EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG
SPL Nonce:              Hr4SV37wbyBMCvDq9hbMU3qKebicuPmSz6AKdTd7ysrD
SPL Ed25519 Signer:     54JfXE4CGxgRsFJkSupJ4kYWFbYauf2Us9GC4FUGCGmS
SPL Message Executor:   3LqtPnGXhqYkXNwoHtWM68t1hUfPrVuAdyxN6CCpUZof
Upgrade authority:      CgnDbeBKoNro2kfhkyWwmsFth7DFHfHDttUay1azbGg4
```

The final deployment will be immutable. Until the current binaries are deployed,
use the local SVM and local-validator tests described in the client plan rather
than the devnet commands below.

## Prerequisites

You need the Solana CLI installed, this repository checked out, and a devnet
keypair with a little SOL to pay fees. The optional devnet USDC section also
uses `spl-token` and `jq`.

Clone the repository, build the CLI, and point a variable at it:

```text
git clone --branch clients-mvp https://github.com/solana-program/ed25519-programmatic-signer.git
cd ed25519-programmatic-signer
make build-clients-cli
PSIGNER=target/debug/psigner
```

If the toolchain is missing, install it first with
`rustup toolchain install nightly-2026-01-22`.

Set up the fee payer. By default this uses the keypair in your Solana CLI
config. Set `FEE_PAYER` first if you want to use a different funded devnet
keypair.

```text
FEE_PAYER="${FEE_PAYER:-$(solana config get | awk -F': ' '/Keypair Path/ {print $2}')}"
solana balance -u devnet --keypair "$FEE_PAYER"
```

If you need funds, go to [faucet](https://faucet.solana.com/). The cold authority never needs SOL.
The fee payer covers everything.

## Step 1: create the demo keys

```text
rm -rf target/devnet-demo
mkdir -p target/devnet-demo

solana-keygen new --silent --no-bip39-passphrase \
  --outfile target/devnet-demo/cold-authority.json

solana-keygen new --silent --no-bip39-passphrase \
  --outfile target/devnet-demo/nonce-account.json

solana-keygen new --silent --no-bip39-passphrase \
  --outfile target/devnet-demo/recipient.json
```

Next run commands below to record the addresses, and derive the cold authority's `ProgrammaticSigner` PDA.
The PDA is the on-chain actor. It holds the funds and signs inside the inner
message through promotion, while the cold key only ever signs transaction
files offline.

```text
COLD_AUTHORITY=$(solana address -k target/devnet-demo/cold-authority.json)
NONCE_ACCOUNT=$(solana address -k target/devnet-demo/nonce-account.json)
RECIPIENT=$(solana address -k target/devnet-demo/recipient.json)
PDA=$($PSIGNER -u devnet address "$COLD_AUTHORITY")

echo "cold authority: $COLD_AUTHORITY"
echo "nonce account:  $NONCE_ACCOUNT"
echo "recipient:      $RECIPIENT"
echo "pda:            $PDA"
```

## Step 2: create the nonce account

Create an SPL Nonce account whose stored authority is the PDA:

```text
$PSIGNER -u devnet nonce create \
  --programmatic-authority "$COLD_AUTHORITY" \
  --nonce-keypair target/devnet-demo/nonce-account.json \
  --fee-payer "$FEE_PAYER"

NONCE=$($PSIGNER -u devnet nonce show "$NONCE_ACCOUNT" | awk -F': ' '/^nonce:/ {print $2}')
echo "$NONCE"
```

Every transaction file is built against this value, and it changes each time one lands.

## Step 3: build the inner SOL transfer with the Solana CLI

Fund the PDA so it has something to send:

```text
solana transfer -u devnet "$PDA" 0.02 \
  --keypair "$FEE_PAYER" \
  --allow-unfunded-recipient
```

Build the inner message with the stock Solana CLI. This example sends native
SOL through the System Program. It uses the classic durable-nonce sign-only
pattern with the nonce value as the blockhash.
The PDA is the sender, which is why the CLI cannot sign it and reports it as
absent:

```text
solana transfer -u devnet "$RECIPIENT" 0.001 \
  --from "$PDA" \
  --fee-payer "$PDA" \
  --blockhash "$NONCE" \
  --sign-only \
  --dump-transaction-message \
  --output json-compact \
  --allow-unfunded-recipient \
  > target/devnet-demo/inner.json
```

`psigner` builds nothing itself here. Any CLI that emits sign-only JSON with a
dumped message works, which is what makes SPL Token Program transfers below
possible with zero extra tooling.

## Step 4: wrap it into a transaction file

```text
$PSIGNER -u devnet transaction create \
  --from-sign-only target/devnet-demo/inner.json \
  --nonce "$NONCE_ACCOUNT" \
  --authority "$COLD_AUTHORITY" \
  --fetch-nonce \
  --outfile target/devnet-demo/tx.psigner
```

`--fetch-nonce` checks the live nonce account while creating the transaction file, so a
stale inner message fails here instead of at submit time.

## Step 5: simulate the inner message

Before asking anyone to sign, check that the transfer would actually succeed
against live state:

```text
$PSIGNER -u devnet transaction simulate inner target/devnet-demo/tx.psigner
```

This runs the inner message as a standalone transaction with signature
verification disabled and a fresh blockhash substituted. It answers "would this
instruction succeed right now" without consuming the nonce or touching the
signer, executor, or nonce programs.

Expected shape:

```text
err: null
units consumed: ...
logs:
  ...
```

Simulation is advisory and online-only. It never replaces the inspect step on
the signing machine.

## Step 6: inspect, then sign

Inspection is the cold-signer safety step, and it needs no network. On a real
deployment this happens on the air-gapped machine before any signature exists:

```text
$PSIGNER transaction inspect target/devnet-demo/tx.psigner
```

Expected shape:

```text
executor program: ExecxgyHYsAXB4c5dZodV1zJZ9hqfsDCYkRDRATrpkFR
genesis hash: EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG
nonce account: $NONCE_ACCOUNT
expected nonce: $NONCE
transaction signers:
  $COLD_AUTHORITY missing
inner required signers:
  $PDA
inner instructions:
  [0] system transfer ...
```

Check the pieces a cold signer would. The genesis hash should be devnet, the
nonce account and expected nonce should match what you created, and the inner
instruction should be the transfer you intended. Then sign.

```text
$PSIGNER transaction sign target/devnet-demo/tx.psigner \
  --keypair target/devnet-demo/cold-authority.json \
  --outfile target/devnet-demo/tx.signed.psigner
```

## Step 7: verify, rehearse, then submit

Before paying the fee, the relayer verifies the signed file against the live
nonce account.

```text
$PSIGNER -u devnet transaction verify \
  target/devnet-demo/tx.signed.psigner \
  --fetch-nonce
```

Then rehearse the real thing. Relay transaction simulation builds the actual
hot relay transaction and runs all three programs, including signature
verification, without paying a fee or consuming the nonce.

```text
$PSIGNER -u devnet transaction simulate \
  relay \
  target/devnet-demo/tx.signed.psigner \
  --fee-payer "$FEE_PAYER"
```

An `err: null` result means the rehearsal passed at the simulated slot. State
can still move before submission, so re-run `verify --fetch-nonce` after any
long delay. Then submit.

```text
$PSIGNER -u devnet transaction submit \
  target/devnet-demo/tx.signed.psigner \
  --fee-payer "$FEE_PAYER"
```

On success it prints the landing signature and the advanced nonce. Confirm the
transfer arrived.

```text
solana balance -u devnet "$RECIPIENT"
```

## Step 8: watch replay protection work

The submit consumed the nonce, so the same transaction file can never land again:

```text
$PSIGNER -u devnet transaction verify \
  target/devnet-demo/tx.signed.psigner \
  --fetch-nonce
```

Expected result:

```text
Error: nonce mismatch
```

Every copy of the transaction file everywhere just became worthless,
atomically with the transfer it executed. That is the property the whole stack exists to
provide.

That is the core loop. Everything below is optional.

> **Before any optional section**: each landed transaction file advances the
> nonce. Re-read it and rebuild the inner message first, or the next file will fail
> with `nonce mismatch`:
>
> ```text
> NONCE=$($PSIGNER -u devnet nonce show "$NONCE_ACCOUNT" | awk -F': ' '/^nonce:/ {print $2}')
> ```

## Optional: cancel a pending transaction file

Cancellation revokes a signed transaction file that has not landed by
consuming its nonce first. The cancellation is itself a transaction file with
an empty inner message, so it walks the same inspect, sign, and submit loop.

The core transfer above already landed, so this copy-paste demo builds a
cancellation file from the current nonce account state. For a real pending file
that has not landed yet, use `nonce advance --from-transaction FILE` instead.

```text
$PSIGNER -u devnet nonce advance \
  --nonce "$NONCE_ACCOUNT" \
  --authority "$COLD_AUTHORITY" \
  --fetch-nonce \
  --outfile target/devnet-demo/cancel.psigner
```

Then inspect, sign, and submit it like any transaction file:

```text
$PSIGNER transaction inspect target/devnet-demo/cancel.psigner

$PSIGNER transaction sign target/devnet-demo/cancel.psigner \
  --keypair target/devnet-demo/cold-authority.json \
  --outfile target/devnet-demo/cancel.signed.psigner

$PSIGNER -u devnet transaction submit \
  target/devnet-demo/cancel.signed.psigner \
  --fee-payer "$FEE_PAYER"
```

The cancellation and the original transaction file both try to consume the same
nonce, so only one of them can land. If the original file lands first, the
cancellation fails with `nonce mismatch`. If the cancellation lands first, the
original file fails the same way. Nothing is revoked until the cancellation
confirms.

## Optional: collect multiple cold signatures

`--authority` accepts several cold authorities. Every listed authority must
sign before the transaction file is submittable.

This example adds a second cold signer as an approval signer on the transaction
file. The inner transfer still spends from the first PDA.

```text
solana-keygen new --silent --no-bip39-passphrase \
  --outfile target/devnet-demo/cold-authority-2.json

COLD_AUTHORITY_2=$(solana address -k target/devnet-demo/cold-authority-2.json)

NONCE=$($PSIGNER -u devnet nonce show "$NONCE_ACCOUNT" | awk -F': ' '/^nonce:/ {print $2}')

solana transfer -u devnet "$RECIPIENT" 0.001 \
  --from "$PDA" \
  --fee-payer "$PDA" \
  --blockhash "$NONCE" \
  --sign-only \
  --dump-transaction-message \
  --output json-compact \
  --allow-unfunded-recipient \
  > target/devnet-demo/inner.multisig.json
```

Wrap that inner message and require both cold authorities to sign the
transaction file:

```text
$PSIGNER -u devnet transaction create \
  --from-sign-only target/devnet-demo/inner.multisig.json \
  --nonce "$NONCE_ACCOUNT" \
  --authority "$COLD_AUTHORITY" "$COLD_AUTHORITY_2" \
  --fetch-nonce \
  --outfile target/devnet-demo/tx.multisig.psigner
```

Each signer signs their own copy, in any order:

```text
$PSIGNER transaction sign target/devnet-demo/tx.multisig.psigner \
  --keypair target/devnet-demo/cold-authority.json \
  --outfile target/devnet-demo/tx.multisig.authority-1.psigner

$PSIGNER transaction sign target/devnet-demo/tx.multisig.psigner \
  --keypair target/devnet-demo/cold-authority-2.json \
  --outfile target/devnet-demo/tx.multisig.authority-2.psigner
```

Merge the copies and proceed as usual:

```text
$PSIGNER transaction merge \
  target/devnet-demo/tx.multisig.authority-1.psigner \
  target/devnet-demo/tx.multisig.authority-2.psigner \
  --outfile target/devnet-demo/tx.multisig.merged.psigner
```

Then verify, simulate, and submit the merged file:

```text
$PSIGNER -u devnet transaction verify \
  target/devnet-demo/tx.multisig.merged.psigner \
  --fetch-nonce

$PSIGNER -u devnet transaction simulate relay \
  target/devnet-demo/tx.multisig.merged.psigner \
  --fee-payer "$FEE_PAYER"

$PSIGNER -u devnet transaction submit \
  target/devnet-demo/tx.multisig.merged.psigner \
  --fee-payer "$FEE_PAYER"
```

## Optional: require a designated relayer

Anyone with the fully signed transaction file can submit it. To restrict
submission to one specific relayer, make that relayer's key a required signer
of the inner message and name it at create time.

The relayer ends up signing twice, and the two signatures do different jobs.
The in-file signature approves the transaction alongside the cold authorities.
The live signature on the outer transaction proves this relayer, and nobody
else, performed the submission — the signer program only forwards a required
signer's privilege when the outer transaction carries its live signature.

Build the inner message with the relayer as fee payer:

```text
NONCE=$($PSIGNER -u devnet nonce show "$NONCE_ACCOUNT" | awk -F': ' '/^nonce:/ {print $2}')
RELAYER=$(solana address -k "$FEE_PAYER")

solana transfer -u devnet "$RECIPIENT" 0.001 \
  --from "$PDA" \
  --fee-payer "$RELAYER" \
  --blockhash "$NONCE" \
  --sign-only \
  --dump-transaction-message \
  --output json-compact \
  --allow-unfunded-recipient \
  > target/devnet-demo/inner-designated.json
```

Wrap it, naming the relayer as a submit signer:

```text
$PSIGNER -u devnet transaction create \
  --from-sign-only target/devnet-demo/inner-designated.json \
  --nonce "$NONCE_ACCOUNT" \
  --authority "$COLD_AUTHORITY" \
  --submit-signer "$RELAYER" \
  --fetch-nonce \
  --outfile target/devnet-demo/tx.designated.psigner
```

The cold authority signs first. The relayer signs the file, rehearses the hot
relay transaction, then submits with its live signature.

```text
$PSIGNER transaction sign target/devnet-demo/tx.designated.psigner \
  --keypair target/devnet-demo/cold-authority.json \
  --outfile target/devnet-demo/tx.designated.cold.psigner

$PSIGNER transaction sign target/devnet-demo/tx.designated.cold.psigner \
  --keypair "$FEE_PAYER" \
  --outfile target/devnet-demo/tx.designated.ready.psigner

$PSIGNER -u devnet transaction verify \
  target/devnet-demo/tx.designated.ready.psigner \
  --fetch-nonce

$PSIGNER -u devnet transaction simulate relay \
  target/devnet-demo/tx.designated.ready.psigner \
  --fee-payer "$FEE_PAYER" \
  --submit-signer "$FEE_PAYER"

$PSIGNER -u devnet transaction submit \
  target/devnet-demo/tx.designated.ready.psigner \
  --fee-payer "$FEE_PAYER" \
  --submit-signer "$FEE_PAYER"
```

Anyone else who obtains the file cannot land it. Their submission fails because
the relayer's live outer signature is missing.

## Optional: SPL Token transfer

The main demo moved native SOL with the System Program. This section moves
devnet USDC with the SPL Token Program. The flow is the same: the SPL Token CLI
builds the inner sign-only message, and `psigner` wraps it.

Circle's Solana Devnet USDC mint is hardcoded here so the example is
copy-pasteable. The faucet sends 20 devnet USDC per request.

```text
USDC_MINT=4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU
USDC_DECIMALS=6
USDC_AMOUNT=1
```

Derive the PDA and recipient associated token accounts:

```text
PDA_USDC=$(spl-token -u devnet address \
  --verbose \
  --output json-compact \
  --token "$USDC_MINT" \
  --owner "$PDA" \
  | jq -r '.associatedTokenAddress')

RECIPIENT_USDC=$(spl-token -u devnet address \
  --verbose \
  --output json-compact \
  --token "$USDC_MINT" \
  --owner "$RECIPIENT" \
  | jq -r '.associatedTokenAddress')

echo "pda usdc account:       $PDA_USDC"
echo "recipient usdc account: $RECIPIENT_USDC"
```

Create the token accounts if they do not exist. These setup transactions are
ordinary online transactions paid by `FEE_PAYER`. The cold authority still does
not need SOL.

```text
spl-token -u devnet account-info --address "$PDA_USDC" >/dev/null 2>&1 || \
  spl-token -u devnet create-account "$USDC_MINT" \
    --owner "$PDA" \
    --fee-payer "$FEE_PAYER"

spl-token -u devnet account-info --address "$RECIPIENT_USDC" >/dev/null 2>&1 || \
  spl-token -u devnet create-account "$USDC_MINT" \
    --owner "$RECIPIENT" \
    --fee-payer "$FEE_PAYER"
```

Open the [Circle faucet](https://faucet.circle.com/), choose Solana Devnet
USDC, and paste the PDA address as the recipient:

```text
echo "$PDA"
open https://faucet.circle.com/
```

After the faucet lands, check that the PDA owns USDC:

```text
spl-token balance -u devnet "$USDC_MINT" --owner "$PDA"
```

Build the USDC transfer as sign-only JSON. The PDA owns the source token
account, so it appears as the absent signer.

```text
NONCE=$($PSIGNER -u devnet nonce show "$NONCE_ACCOUNT" | awk -F': ' '/^nonce:/ {print $2}')

spl-token transfer -u devnet "$USDC_MINT" "$USDC_AMOUNT" "$RECIPIENT_USDC" \
  --from "$PDA_USDC" \
  --owner "$PDA" \
  --fee-payer "$PDA" \
  --blockhash "$NONCE" \
  --mint-decimals "$USDC_DECIMALS" \
  --sign-only \
  --dump-transaction-message \
  --output json-compact \
  > target/devnet-demo/usdc-inner.json
```

Wrap it into a transaction file:

```text
$PSIGNER -u devnet transaction create \
  --from-sign-only target/devnet-demo/usdc-inner.json \
  --nonce "$NONCE_ACCOUNT" \
  --authority "$COLD_AUTHORITY" \
  --fetch-nonce \
  --outfile target/devnet-demo/usdc.psigner
```

Simulate the inner SPL Token transfer before asking the cold authority to sign:

```text
$PSIGNER -u devnet transaction simulate inner target/devnet-demo/usdc.psigner
```

Inspect the USDC transfer before signing:

```text
$PSIGNER transaction inspect target/devnet-demo/usdc.psigner
```

Sign it with the cold authority:

```text
$PSIGNER transaction sign target/devnet-demo/usdc.psigner \
  --keypair target/devnet-demo/cold-authority.json \
  --outfile target/devnet-demo/usdc.signed.psigner
```

Verify against the live nonce account, rehearse the hot relay transaction, and
submit:

```text
$PSIGNER -u devnet transaction verify \
  target/devnet-demo/usdc.signed.psigner \
  --fetch-nonce

$PSIGNER -u devnet transaction simulate relay \
  target/devnet-demo/usdc.signed.psigner \
  --fee-payer "$FEE_PAYER"

$PSIGNER -u devnet transaction submit \
  target/devnet-demo/usdc.signed.psigner \
  --fee-payer "$FEE_PAYER"
```

Confirm the recipient received devnet USDC:

```text
spl-token balance -u devnet "$USDC_MINT" --owner "$RECIPIENT"
```

## Optional: custom program instructions

If a program does not have a CLI that emits Solana sign-only JSON, use a small
script or app to build the inner message. This example uses the Memo program as
a harmless stand-in for a custom program. The memo instruction requires the PDA
as signer and writes the memo into transaction logs.

Build the inner sign-only JSON with the Rust example. This writes only the
source message dump, not a psigner transaction file yet.

```text
NONCE=$($PSIGNER -u devnet nonce show "$NONCE_ACCOUNT" | awk -F': ' '/^nonce:/ {print $2}')

cargo run -q -p spl-programmatic-signer-rust --example build_memo_inner -- \
  "$PDA" \
  "$NONCE" \
  "psigner custom memo demo" \
  > target/devnet-demo/memo-inner.json
```

The generated JSON has the same shape as Solana and SPL Token sign-only output:

```json
{
  "blockhash": "NONCE_VALUE",
  "message": "BASE64_SERIALIZED_VERSIONED_MESSAGE",
  "absent": ["PROGRAMMATIC_SIGNER_PDA"]
}
```

Wrap the memo message into `target/devnet-demo/memo.psigner`. The simulate,
inspect, sign, verify, and submit commands below all use this wrapped
transaction file.

```text
$PSIGNER -u devnet transaction create \
  --from-sign-only target/devnet-demo/memo-inner.json \
  --nonce "$NONCE_ACCOUNT" \
  --authority "$COLD_AUTHORITY" \
  --fetch-nonce \
  --outfile target/devnet-demo/memo.psigner
```

Simulate the inner memo. This confirms the Memo program sees the PDA as signed:

```text
$PSIGNER -u devnet transaction simulate inner target/devnet-demo/memo.psigner
```

Expected logs include:

```text
Program log: Signed by $PDA
Program log: Memo (len 24): "psigner custom memo demo"
```

Inspect, sign, verify, rehearse, and submit:

```text
$PSIGNER transaction inspect target/devnet-demo/memo.psigner

$PSIGNER transaction sign target/devnet-demo/memo.psigner \
  --keypair target/devnet-demo/cold-authority.json \
  --outfile target/devnet-demo/memo.signed.psigner

$PSIGNER -u devnet transaction verify \
  target/devnet-demo/memo.signed.psigner \
  --fetch-nonce

$PSIGNER -u devnet transaction simulate relay \
  target/devnet-demo/memo.signed.psigner \
  --fee-payer "$FEE_PAYER"

$PSIGNER -u devnet transaction submit \
  target/devnet-demo/memo.signed.psigner \
  --fee-payer "$FEE_PAYER"
```

For a real custom program, the builder changes but the handoff does not. Build
the instruction with the custom program id, accounts, and instruction bytes; use
the ProgrammaticSigner PDA anywhere the custom program expects signer
authority; serialize the inner message into the same sign-only JSON shape; then
run the same `transaction create` flow. Inspection may show raw program id,
account keys, and instruction data until a decoder is added, so the cold signer
still needs a trusted way to understand those bytes before signing.

## Optional: hardware wallets

The main guide uses local keypair files so every command is easy to copy and
paste. For a real cold authority, use a hardware wallet through Solana's
remote-wallet URL format.

First derive the cold authority address from the hardware wallet.

```text
COLD_AUTHORITY=$(solana address -k 'usb://ledger?key=0')
PDA=$($PSIGNER -u devnet address "$COLD_AUTHORITY")

echo "cold authority: $COLD_AUTHORITY"
echo "pda:            $PDA"
```

Then follow the same flow as the local-keypair example. The key difference is
that every place the guide passes the cold-authority keypair file, pass the
hardware-wallet URL instead.

```text
$PSIGNER transaction inspect target/devnet-demo/tx.psigner

$PSIGNER transaction sign target/devnet-demo/tx.psigner \
  --keypair 'usb://ledger?key=0' \
  --outfile target/devnet-demo/tx.signed.psigner
```

Quote the URL in zsh. Inspect before hardware signing. The Ledger app requires
blind signing to be enabled because the cold-signed transaction is opaque to it.

Signer flags that produce signatures accept the same URL shape. This applies to
`--keypair`, `--fee-payer`, and `--submit-signer`. `--nonce-keypair` stays
file-only because it creates the new nonce account address.

## Troubleshooting

- `nonce mismatch` after something landed is not a bug. It is the replay
  protection working. Re-read the nonce and rebuild the inner message.
- Always pass `-u devnet` on commands that touch the network. Offline commands
  (`inspect`, `sign`, `merge`) need no flag at all.
- Insufficient funds at submit means the fee payer, not the cold authority,
  needs SOL. The cold authority never holds any.
- One nonce account is one serial lane. Two transaction files on the same
  account exclude each other by design. For parallel submissions, create one
  nonce account per in-flight transaction file under the same authority.
- Devnet RPC can serve stale account data right after confirmation. The CLI
  retries post-confirm nonce reads for `nonce create` and `transaction submit`.
