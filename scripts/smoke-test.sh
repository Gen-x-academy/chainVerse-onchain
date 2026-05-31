#!/bin/bash
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
DEPLOYMENTS_FILE="${DEPLOYMENTS_FILE:-$ROOT_DIR/deployments/testnet.json}"
STELLAR_NETWORK="${STELLAR_NETWORK:-testnet}"

if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq is required" >&2
  exit 127
fi

if ! command -v stellar >/dev/null 2>&1; then
  echo "error: stellar CLI is required (see scripts/setup-testnet-account.sh)" >&2
  exit 127
fi

if [[ ! -f "$DEPLOYMENTS_FILE" ]]; then
  echo "error: deployments file not found: $DEPLOYMENTS_FILE" >&2
  echo "expected JSON shape: { \"contracts\": { \"escrow\": \"<contract_id>\", ... } }" >&2
  exit 2
fi

readarray -t CONTRACT_ROWS < <(
  jq -r '
    def contracts_obj:
      if (.contracts? and (.contracts|type) == "object") then .contracts else . end;
    contracts_obj
    | to_entries[]
    | select(.value|type == "string")
    | "\(.key)\t\(.value)"
  ' "$DEPLOYMENTS_FILE"
)

if [[ "${#CONTRACT_ROWS[@]}" -eq 0 ]]; then
  echo "error: no contracts found in $DEPLOYMENTS_FILE" >&2
  exit 3
fi

SOURCE_ACCOUNT_FLAGS=()
if [[ -n "${SOURCE_ACCOUNT:-}" ]]; then
  SOURCE_ACCOUNT_FLAGS=(--source-account "$SOURCE_ACCOUNT")
fi

invoke() {
  local contract_id="$1"
  local function_name="$2"
  stellar contract invoke \
    --network "$STELLAR_NETWORK" \
    --id "$contract_id" \
    "${SOURCE_ACCOUNT_FLAGS[@]}" \
    -- "$function_name"
}

failures=0
for row in "${CONTRACT_ROWS[@]}"; do
  contract_name="${row%%$'\t'*}"
  contract_id="${row#*$'\t'}"

  echo "==> $contract_name ($contract_id)"

  primary_fn="version"
  case "$contract_name" in
    *escrow*)
      primary_fn="get_escrow_count"
      ;;
  esac

  if output="$(invoke "$contract_id" "$primary_fn" 2>&1)"; then
    if [[ -z "${output//[[:space:]]/}" || "$output" == "null" ]]; then
      echo "error: $contract_name responded with empty/null for $primary_fn" >&2
      failures=$((failures + 1))
      continue
    fi
    echo "$primary_fn -> $output"
    continue
  fi

  if output2="$(invoke "$contract_id" "get_admin" 2>&1)"; then
    if [[ -z "${output2//[[:space:]]/}" || "$output2" == "null" ]]; then
      echo "error: $contract_name responded with empty/null for get_admin" >&2
      failures=$((failures + 1))
      continue
    fi
    echo "get_admin -> $output2"
    continue
  fi

  echo "error: $contract_name smoke test failed" >&2
  echo "primary ($primary_fn) error:" >&2
  echo "$output" >&2
  echo "fallback (get_admin) error:" >&2
  echo "$output2" >&2
  failures=$((failures + 1))
done

if [[ "$failures" -ne 0 ]]; then
  echo "smoke test failed: $failures contract(s) not responding" >&2
  exit 1
fi

echo "smoke test passed: ${#CONTRACT_ROWS[@]} contract(s) responding"
