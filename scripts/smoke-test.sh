#!/bin/bash
# scripts/smoke-test.sh
#
# Smoke-test all deployed ChainVerse contracts on Stellar testnet.
# Validates contract IDs (StrKey format) and network passphrase before invoking.
#
# Usage:
#   ./scripts/smoke-test.sh [DEPLOYMENT_FILE]
#
# Exit codes:
#   0 — all smoke checks passed (or were skipped due to missing addresses)
#   1 — one or more checks failed or the deployment file was not found

set -euo pipefail

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------
DEPLOYMENT_FILE="${1:-deployments/testnet.json}"
NETWORK="${STELLAR_NETWORK:-testnet}"
IDENTITY="${STELLAR_IDENTITY:-deployer}"
PASS=0
FAIL=0
SKIP=0

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

# Validate a contract ID is in StrKey format (starts with C, 56 chars).
validate_contract_id() {
  local id="$1"
  local name="$2"
  if [[ ! "$id" =~ ^C[A-Z0-9]{55}$ ]]; then
    echo "  [FAIL] $name: invalid contract ID format: $id" >&2
    FAIL=$((FAIL + 1))
    return 1
  fi
  return 0
}

# Extract a contract address from the JSON deployment file.
get_address() {
  local key="$1"
  # Support both old flat format and new nested format with "address" field
  local addr
  addr=$(grep -o "\"${key}\": *\"C[A-Z0-9]\{55\}\"" "$DEPLOYMENT_FILE" \
    | grep -o '"C[A-Z0-9]*"' \
    | tr -d '"' || true)
  if [ -z "$addr" ]; then
    addr=$(grep -o "\"${key}\": *{[^}]*\"address\": *\"C[A-Z0-9]\{55\}\"" "$DEPLOYMENT_FILE" \
      | grep -o '"address": *"[^"]*"' \
      | grep -o '"C[A-Z0-9]*"' \
      | tr -d '"' || true)
  fi
  echo "$addr"
}

# Invoke a single read-only function and track pass / fail.
check_fn() {
  local display="$1"
  local json_key="$2"
  local fn_name="$3"
  shift 3
  local extra_args=("$@")

  local contract_id
  contract_id=$(get_address "$json_key")

  if [ -z "$contract_id" ]; then
    echo "  [SKIP] $display — no address in $DEPLOYMENT_FILE"
    SKIP=$((SKIP + 1))
    return
  fi

  if ! validate_contract_id "$contract_id" "$display"; then
    return
  fi

  printf "  %-45s ... " "$display ($fn_name)"
  local output
  if output=$(stellar contract invoke \
        --id    "$contract_id" \
        --source "$IDENTITY" \
        --network "$NETWORK" \
        -- "$fn_name" "${extra_args[@]}" 2>&1); then
    echo "PASS  →  ${output}"
    PASS=$((PASS + 1))
  else
    echo "FAIL"
    echo "      error: ${output}"
    FAIL=$((FAIL + 1))
  fi
}

# ---------------------------------------------------------------------------
# Pre-flight
# ---------------------------------------------------------------------------
if [ ! -f "$DEPLOYMENT_FILE" ]; then
  echo "ERROR: deployment file not found: $DEPLOYMENT_FILE"
  exit 1
fi

# Validate network passphrase before any mutations
if [ "$NETWORK" = "testnet" ]; then
  EXPECTED_PASSPHRASE="Test SDF Network ; September 2015"
  ACTUAL_PASSPHRASE=$(stellar network passphrase --network testnet 2>/dev/null || echo "")
  if [ -n "$ACTUAL_PASSPHRASE" ] && [ "$ACTUAL_PASSPHRASE" != "$EXPECTED_PASSPHRASE" ]; then
    echo "ERROR: network passphrase mismatch" >&2
    echo "  expected: $EXPECTED_PASSPHRASE" >&2
    echo "  actual:   $ACTUAL_PASSPHRASE" >&2
    exit 1
  fi
fi

echo "======================================================="
echo " ChainVerse Smoke Test"
echo " Network  : $NETWORK"
echo " Identity : $IDENTITY"
echo " File     : $DEPLOYMENT_FILE"
echo "======================================================="

# ---------------------------------------------------------------------------
# CHV Token
# ---------------------------------------------------------------------------
echo ""
echo "--- CHV Token ---"
check_fn "CHV Token: total supply minted" "chv_token" "total_minted"

# ---------------------------------------------------------------------------
# Certificates
# ---------------------------------------------------------------------------
echo ""
echo "--- Certificates ---"
check_fn "Certificates: is_paused" "certificates" "is_paused"

# ---------------------------------------------------------------------------
# Escrow
# ---------------------------------------------------------------------------
echo ""
echo "--- Escrow ---"
check_fn "Escrow: get_escrow" "escrow" "get_escrow" --escrow_id 0

# ---------------------------------------------------------------------------
# Escrow Vault
# ---------------------------------------------------------------------------
echo ""
echo "--- Escrow Vault ---"
escrow_vault_id=$(get_address "escrow_vault")
if [ -z "$escrow_vault_id" ]; then
  echo "  [SKIP] Escrow Vault — no address in $DEPLOYMENT_FILE"
  SKIP=$((SKIP + 1))
else
  if validate_contract_id "$escrow_vault_id" "Escrow Vault"; then
    echo "  [INFO] Escrow Vault deployed at $escrow_vault_id (no zero-arg read to invoke)"
  fi
fi

# ---------------------------------------------------------------------------
# ChainVerse Core
# ---------------------------------------------------------------------------
echo ""
echo "--- ChainVerse Core ---"
check_fn "ChainVerse Core: is_paused" "chainverse_core" "is_paused"
check_fn "ChainVerse Core: version"   "chainverse_core" "version"

# ---------------------------------------------------------------------------
# Reward
# ---------------------------------------------------------------------------
echo ""
echo "--- Reward ---"
check_fn "Reward: is_paused"          "reward" "is_paused"
check_fn "Reward: get_backend_pubkey" "reward" "get_backend_pubkey"

# ---------------------------------------------------------------------------
# Course Registry
# ---------------------------------------------------------------------------
echo ""
echo "--- Course Registry ---"
check_fn "Course Registry: is_paused" "course_registry" "is_paused"
check_fn "Course Registry: version"   "course_registry" "version"

# ---------------------------------------------------------------------------
# Payout Automation
# ---------------------------------------------------------------------------
echo ""
echo "--- Payout Automation ---"
payout_id=$(get_address "payout_automation")
if [ -z "$payout_id" ]; then
  echo "  [SKIP] Payout Automation — no address in $DEPLOYMENT_FILE"
  SKIP=$((SKIP + 1))
else
  if validate_contract_id "$payout_id" "Payout Automation"; then
    echo "  [INFO] Payout Automation deployed at $payout_id (no zero-arg read to invoke)"
  fi
fi

# ---------------------------------------------------------------------------
# Staking
# ---------------------------------------------------------------------------
echo ""
echo "--- Staking ---"
staking_id=$(get_address "staking")
if [ -z "$staking_id" ]; then
  echo "  [SKIP] Staking — no address in $DEPLOYMENT_FILE"
  SKIP=$((SKIP + 1))
else
  if validate_contract_id "$staking_id" "Staking"; then
    echo "  [INFO] Staking deployed at $staking_id (no zero-arg read to invoke)"
  fi
fi

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo ""
echo "======================================================="
echo " Results: ${PASS} passed  |  ${FAIL} failed  |  ${SKIP} skipped"
echo "======================================================="

if [ "$FAIL" -gt 0 ]; then
  echo "SMOKE TEST FAILED — review errors above."
  exit 1
fi

echo "All smoke tests passed!"
exit 0
