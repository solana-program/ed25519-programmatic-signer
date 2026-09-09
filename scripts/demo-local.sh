#!/usr/bin/env bash
# Rehearse the current ABI using a private, disposable local validator and demo keys.
set -euo pipefail
umask 077
cd "$(dirname "$0")/.."
repo_dir="$PWD"
for tool in solana solana-keygen solana-test-validator spl-token jq curl python3; do
  command -v "$tool" >/dev/null || { echo "Missing required tool: $tool" >&2; exit 1; }
done
rpc_port="${DEMO_RPC_PORT:-18899}"
python3 - "$rpc_port" <<'PY'
import socket, sys
port = int(sys.argv[1])
if not 1024 <= port <= 65532:
    raise SystemExit('DEMO_RPC_PORT must be between 1024 and 65532')
sockets = []
for p in range(port, port + 3):
    s = socket.socket()
    try:
        s.bind(('127.0.0.1', p))
    except OSError as e:
        raise SystemExit(f'Local demo port {p} is unavailable: {e}')
    sockets.append(s)
PY
mkdir -p target/local-demo
demo_dir="$(mktemp -d "$repo_dir/target/local-demo/run.XXXXXX")"
cli="$repo_dir/target/debug/spl-programmatic-signer-cli"
url="http://127.0.0.1:$rpc_port"
for key in payer cold recipient mint source-token destination-token; do
  solana-keygen new --no-bip39-passphrase --silent --outfile "$demo_dir/$key.json" >/dev/null
done
payer="$(solana-keygen pubkey "$demo_dir/payer.json")"
cold="$(solana-keygen pubkey "$demo_dir/cold.json")"
recipient="$(solana-keygen pubkey "$demo_dir/recipient.json")"
cat > "$demo_dir/config.yml" <<YAML
json_rpc_url: $url
websocket_url: ws://127.0.0.1:$((rpc_port + 1))
keypair_path: $demo_dir/payer.json
address_labels: {}
commitment: confirmed
YAML
validator_pid=''
cleanup() {
  if [[ -n "$validator_pid" ]]; then
    kill "$validator_pid" 2>/dev/null || true
    wait "$validator_pid" 2>/dev/null || true
  fi
  echo "Demo artifacts: $demo_dir"
}
trap cleanup EXIT
trap 'exit 130' INT TERM
solana-test-validator --quiet --ledger "$demo_dir/ledger" \
  --bind-address 127.0.0.1 --rpc-port "$rpc_port" --faucet-port "$((rpc_port + 2))" \
  --mint "$payer" \
  --bpf-program Noncediea1fH12usShuQAz28UhgAeuE5Maf32LsMUQB target/deploy/spl_nonce_program.so \
  --bpf-program EdSigVfK1DkeMrjFNDMjwfQaJPhPTtX7jW8uPv3oKEgN target/deploy/spl_ed25519_signer_program.so \
  --bpf-program ExecxgyHYsAXB4c5dZodV1zJZ9hqfsDCYkRDRATrpkFR target/deploy/spl_legacy_message_executor_program.so \
  > "$demo_dir/validator.log" 2>&1 &
validator_pid=$!
ready=false
for ((attempt=0; attempt<90; attempt++)); do
  kill -0 "$validator_pid" 2>/dev/null || { cat "$demo_dir/validator.log" >&2; exit 1; }
  if [[ "$(curl --silent --max-time 1 "$url/health" || true)" == ok ]]; then
    ready=true; break
  fi
  sleep 1
done
[[ "$ready" == true ]] || { echo 'Local validator did not become ready' >&2; exit 1; }
psigner() { "$cli" -C "$demo_dir/config.yml" "$@"; }
sol() { solana -C "$demo_dir/config.yml" "$@"; }
token() { spl-token -u "$url" -C "$demo_dir/config.yml" "$@"; }
pda="$(psigner address "$cold")"
psigner --output json-compact nonce create --cold-authority "$cold" > "$demo_dir/nonce.json"
nonce_account="$(jq -er .nonceAccount "$demo_dir/nonce.json")"
nonce_value="$(jq -er .nonce "$demo_dir/nonce.json")"
sol transfer "$pda" 0.1 --allow-unfunded-recipient >/dev/null
sol transfer "$recipient" 0.001 --allow-unfunded-recipient >/dev/null
source_sol() {
  sol transfer "$recipient" 0.001 --from "$pda" --fee-payer "$pda" \
    --blockhash "$2" --sign-only --dump-transaction-message --output json-compact \
    --allow-unfunded-recipient > "$demo_dir/$1.json"
}
wrap() {
  local name="$1"; shift
  psigner transaction create --from-sign-only "$demo_dir/$name.json" \
    --nonce "$nonce_account" --authority "$cold" --outfile "$demo_dir/$name.psigner" "$@"
}
reject_submit() {
  if psigner transaction submit "$1" > "$demo_dir/rejected.log" 2>&1; then
    echo 'Expected stale nonce rejection, but submission succeeded' >&2; exit 1
  fi
  if ! python3 - "$demo_dir/rejected.log" <<'PY'
import pathlib, sys
raise SystemExit(0 if 'nonce mismatch' in pathlib.Path(sys.argv[1]).read_text() else 1)
PY
  then cat "$demo_dir/rejected.log" >&2; exit 1; fi
}
echo 'Create and cold-sign two dependent SOL transfers before submitting either.'
source_sol a "$nonce_value"
wrap a --fetch-nonce
next="$(psigner transaction next-nonce "$demo_dir/a.psigner")"
source_sol b "$next"
wrap b --after "$demo_dir/a.psigner" -u http://127.0.0.1:1
psigner transaction inspect "$demo_dir/a.psigner" -u http://127.0.0.1:1
mkdir "$demo_dir/signed"
psigner transaction sign "$demo_dir/a.psigner" "$demo_dir/b.psigner" \
  --keypair "$demo_dir/cold.json" --outdir "$demo_dir/signed" -u http://127.0.0.1:1
reject_submit "$demo_dir/signed/b.psigner"
psigner transaction simulate relay "$demo_dir/signed/a.psigner"
psigner --output json-compact transaction submit "$demo_dir/signed/a.psigner" > "$demo_dir/submitted-a.json"
psigner --output json-compact transaction submit "$demo_dir/signed/b.psigner" > "$demo_dir/submitted-b.json"
jq -e '.expectedNextNonce == .observedNonce' "$demo_dir/submitted-a.json" "$demo_dir/submitted-b.json" >/dev/null
reject_submit "$demo_dir/signed/a.psigner"
echo 'Cancel a pending transfer by submitting an empty inner message first.'
nonce_value="$(jq -er .observedNonce "$demo_dir/submitted-b.json")"
source_sol pending "$nonce_value"
wrap pending --fetch-nonce
psigner transaction sign "$demo_dir/pending.psigner" --keypair "$demo_dir/cold.json" \
  --outfile "$demo_dir/pending.signed.psigner" -u http://127.0.0.1:1
psigner nonce advance --from-transaction "$demo_dir/pending.psigner" --authority "$cold" \
  --outfile "$demo_dir/cancel.psigner" -u http://127.0.0.1:1
psigner transaction sign "$demo_dir/cancel.psigner" --keypair "$demo_dir/cold.json" \
  --outfile "$demo_dir/cancel.signed.psigner" -u http://127.0.0.1:1
psigner transaction submit "$demo_dir/cancel.signed.psigner"
reject_submit "$demo_dir/pending.signed.psigner"
echo 'Create a local mint and import an actual spl-token sign-only transfer.'
mint="$(solana-keygen pubkey "$demo_dir/mint.json")"
source_token="$(solana-keygen pubkey "$demo_dir/source-token.json")"
destination_token="$(solana-keygen pubkey "$demo_dir/destination-token.json")"
token create-token "$demo_dir/mint.json" --decimals 6 --fee-payer "$demo_dir/payer.json" --mint-authority "$payer" >/dev/null
token create-account "$mint" "$demo_dir/source-token.json" --owner "$pda" --fee-payer "$demo_dir/payer.json" >/dev/null
token create-account "$mint" "$demo_dir/destination-token.json" --owner "$recipient" --fee-payer "$demo_dir/payer.json" >/dev/null
token mint "$mint" 2 "$source_token" --fee-payer "$demo_dir/payer.json" --mint-authority "$demo_dir/payer.json" >/dev/null
nonce_value="$(psigner --output json-compact nonce show "$nonce_account" | jq -er .nonce)"
token transfer "$mint" 1.25 "$destination_token" --from "$source_token" \
  --owner "$pda" --fee-payer "$pda" --blockhash "$nonce_value" --mint-decimals 6 \
  --sign-only --dump-transaction-message --output json-compact > "$demo_dir/token.json"
wrap token --fetch-nonce
psigner transaction inspect "$demo_dir/token.psigner" -u http://127.0.0.1:1
psigner transaction simulate inner "$demo_dir/token.psigner"
psigner transaction sign "$demo_dir/token.psigner" --keypair "$demo_dir/cold.json" \
  --outfile "$demo_dir/token.signed.psigner" -u http://127.0.0.1:1
psigner transaction simulate relay "$demo_dir/token.signed.psigner"
psigner transaction submit "$demo_dir/token.signed.psigner"
[[ "$(token balance --address "$destination_token")" == 1.25 ]]
[[ "$(token balance --address "$source_token")" == 0.75 ]]
[[ "$(sol balance "$recipient" --lamports)" == '3000000 lamports' ]]
echo 'Local demo passed: SOL chain, replay rejection, cancellation, and SPL Token transfer.'
