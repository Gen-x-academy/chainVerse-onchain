#!/bin/bash
# scripts/smoke-test.sh
#
# Smoke-test all deployed ChainVerse contracts on Stellar testnet.
#
# Usage:
#   ./scripts/smoke-test.sh [DEPLOYMENT_FILE]
#
# The optional DEPLOYMENT_FILE argument lets you point at any JSON file that
# matches the schema of deployments/testnet.json (useful for staging or a
# local Quickstart node).  It defaults to deployments/testnet.json.
#
# Prerequisites:
#   - stellar CLI installed and on $PATH
#   - A funded testnet identity called "deployer" (or set STELLAR_IDENTITY)
#   - Contract addresses populated in the deployment file
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

# Extract a contract address from the JSON deployment file.
get_address() {
  local key="$1"
  grep -o "\"${key}\": \"[^\"]*\"" "$DEPLOYMENT_FILE" \
    | grep -o '"[^"]*"$' \
    | tr -d '"'
}

# Invoke a single read-only function and track pass / fail.
# Arguments: <display_name> <contract_key_in_json> <function_name> [extra_args...]
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

echo "======================================================="
echo " ChainVerse Smoke Test"
echo " Network  : $NETWORK"
echo " Identity : $IDENTITY"
echo " File     : $DEPLOYMENT_FILE"
echo "======================================================="

# ---------------------------------------------------------------------------
# CHV Token  (contracts/chv_token)
# Read-only, zero-arg functions: total_minted
# ---------------------------------------------------------------------------
echo ""
echo "--- CHV Token ---"
check_fn "CHV Token: total supply minted" "chv_token" "total_minted"

# ---------------------------------------------------------------------------
# Certificates  (contracts/certificates)
# Read-only, zero-arg functions: is_paused
# ---------------------------------------------------------------------------
echo ""
echo "--- Certificates ---"
check_fn "Certificates: is_paused" "certificates" "is_paused"

# ---------------------------------------------------------------------------
# Escrow  (contracts/escrow)
# The escrow contract has no zero-arg read functions.  We verify liveness
# with get_escrow on an id that will not exist yet — a live contract returns
# `None` (not an error), while an unreachable/undeployed contract fails.
# ---------------------------------------------------------------------------
echo ""
echo "--- Escrow ---"
check_fn "Escrow: get_escrow" "escrow" "get_escrow" --escrow_id 0

# ---------------------------------------------------------------------------
# Escrow Vault  (contracts/escrow-vault)
# No side-effect-free zero-arg reads exist; verify the contract address is
# present in the deployment file instead of invoking a state-changing fn.
# ---------------------------------------------------------------------------
echo ""
echo "--- Escrow Vault ---"
escrow_vault_id=$(get_address "escrow_vault")
if [ -z "$escrow_vault_id" ]; then
  echo "  [SKIP] Escrow Vault — no address in $DEPLOYMENT_FILE"
  SKIP=$((SKIP + 1))
else
  echo "  [INFO] Escrow Vault deployed at $escrow_vault_id (no zero-arg read to invoke)"
fi

# ---------------------------------------------------------------------------
# ChainVerse Core  (contracts/chainverse-core)
# Read-only, zero-arg functions: is_paused, version
# ---------------------------------------------------------------------------
echo ""
echo "--- ChainVerse Core ---"
check_fn "ChainVerse Core: is_paused" "chainverse_core" "is_paused"
check_fn "ChainVerse Core: version"   "chainverse_core" "version"

# ---------------------------------------------------------------------------
# Reward  (contracts/reward)
# Read-only, zero-arg functions: is_paused, get_backend_pubkey
# ---------------------------------------------------------------------------
echo ""
echo "--- Reward ---"
check_fn "Reward: is_paused"          "reward" "is_paused"
check_fn "Reward: get_backend_pubkey" "reward" "get_backend_pubkey"

# ---------------------------------------------------------------------------
# Course Registry  (contracts/course_registry)
# Read-only, zero-arg functions: is_paused, version
# ---------------------------------------------------------------------------
echo ""
echo "--- Course Registry ---"
check_fn "Course Registry: is_paused" "course_registry" "is_paused"
check_fn "Course Registry: version"   "course_registry" "version"

# ---------------------------------------------------------------------------
# Payout Automation  (contracts/payout-automation)
# No zero-arg read functions exist; verify the contract address is set.
# ---------------------------------------------------------------------------
echo ""
echo "--- Payout Automation ---"
payout_id=$(get_address "payout_automation")
if [ -z "$payout_id" ]; then
  echo "  [SKIP] Payout Automation — no address in $DEPLOYMENT_FILE"
  SKIP=$((SKIP + 1))
else
  echo "  [INFO] Payout Automation deployed at $payout_id (no zero-arg read to invoke)"
fi

# ---------------------------------------------------------------------------
# Staking  (contracts/staking)
# No zero-arg read functions; verify address is present in deployment file.
# ---------------------------------------------------------------------------
echo ""
echo "--- Staking ---"
staking_id=$(get_address "staking")
if [ -z "$staking_id" ]; then
  echo "  [SKIP] Staking — no address in $DEPLOYMENT_FILE"
  SKIP=$((SKIP + 1))
else
  echo "  [INFO] Staking deployed at $staking_id (no zero-arg read to invoke)"
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
