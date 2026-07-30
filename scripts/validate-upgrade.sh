#!/bin/bash
# scripts/validate-upgrade.sh
#
# Pre-upgrade storage and ABI compatibility validator for ChainVerse contracts.
#
# Fetches the currently deployed WASM for a contract, inspects both the
# deployed and candidate specs, and rejects the upgrade if:
#   - Any function present in the deployed contract is absent from the new WASM
#     (removing a function is a breaking ABI change).
#   - The STORAGE_VERSION constant in the new WASM is lower than the one in the
#     currently deployed WASM (downgrading storage layout is not allowed).
#
# Usage:
#   ./scripts/validate-upgrade.sh <network> <source> <contract-name> <new-wasm-path>
#
# Arguments (positional, same order as upgrade-contract.sh):
#   network        — Stellar network name (testnet | mainnet)
#   source         — Stellar identity/key name (e.g. deployer)
#   contract-name  — Key in deployments/<network>.json (e.g. chv_token)
#   new-wasm-path  — Path to the new .wasm file to validate
#
# Exit codes:
#   0 — all checks passed, safe to upgrade
#   1 — one or more checks failed; upgrade should be aborted

set -euo pipefail

# ---------------------------------------------------------------------------
# Arguments
# ---------------------------------------------------------------------------
NETWORK="${1:-}"
SOURCE="${2:-}"
CONTRACT_NAME="${3:-}"
NEW_WASM="${4:-}"

if [ -z "$NETWORK" ] || [ -z "$CONTRACT_NAME" ] || [ -z "$NEW_WASM" ]; then
  echo "Usage: $0 <network> <source> <contract-name> <new-wasm-path>"
  echo ""
  echo "Examples:"
  echo "  $0 testnet deployer chv_token ./target/wasm32-unknown-unknown/release/chv_token.wasm"
  echo "  $0 mainnet admin    escrow    ./target/wasm32-unknown-unknown/release/escrow.wasm"
  exit 1
fi

# ---------------------------------------------------------------------------
# Derived paths
# ---------------------------------------------------------------------------
DEPLOYMENTS_FILE="deployments/${NETWORK}.json"
CURRENT_WASM="/tmp/current_contract_${CONTRACT_NAME}.wasm"
SPEC_CURRENT="/tmp/spec_current_${CONTRACT_NAME}.txt"
SPEC_NEW="/tmp/spec_new_${CONTRACT_NAME}.txt"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

# Extract a sorted, newline-separated list of function names from an inspect
# output file.  stellar contract inspect prints lines like:
#   fn <name>(...) -> ...
# We capture the first token after "fn " on each such line.
extract_functions() {
  local spec_file="$1"
  grep -E '^\s*fn ' "$spec_file" \
    | sed 's/^\s*fn \([a-zA-Z_][a-zA-Z0-9_]*\).*/\1/' \
    | sort -u
}

# Extract the numeric value of a STORAGE_VERSION or storage_version entry from
# an inspect output file.  The inspect output for a constant looks like:
#   const STORAGE_VERSION: u32 = 3;
# We accept any casing and any unsigned integer type.
extract_storage_version() {
  local spec_file="$1"
  grep -iE '\bSTORAGE_VERSION\b' "$spec_file" \
    | grep -oE '[0-9]+' \
    | head -1 || echo ""
}

# ---------------------------------------------------------------------------
# Preflight
# ---------------------------------------------------------------------------
echo "========================================================="
echo " ChainVerse Upgrade Validator"
echo " Network  : $NETWORK"
echo " Contract : $CONTRACT_NAME"
echo " New WASM : $NEW_WASM"
echo "========================================================="
echo ""

# Verify the new WASM exists before doing anything else.
if [ ! -f "$NEW_WASM" ]; then
  echo "[ERROR] New WASM file not found: $NEW_WASM"
  exit 1
fi

# Verify the deployments file exists.
if [ ! -f "$DEPLOYMENTS_FILE" ]; then
  echo "[ERROR] Deployments file not found: $DEPLOYMENTS_FILE"
  echo "        Run ./scripts/deploy-testnet.sh first, or check the network name."
  exit 1
fi

# ---------------------------------------------------------------------------
# Resolve contract ID
# ---------------------------------------------------------------------------
CONTRACT_ID=$(python3 -c "
import json, sys
with open('$DEPLOYMENTS_FILE') as f:
    data = json.load(f)
cid = data.get('contracts', {}).get('$CONTRACT_NAME', '')
print(cid)
")

if [ -z "$CONTRACT_ID" ]; then
  echo "[ERROR] Contract '$CONTRACT_NAME' not found in $DEPLOYMENTS_FILE"
  echo "        Available contracts:"
  python3 -c "
import json
with open('$DEPLOYMENTS_FILE') as f:
    data = json.load(f)
for k in data.get('contracts', {}):
    print('    -', k)
"
  exit 1
fi

echo "[INFO] Resolved contract ID : $CONTRACT_ID"
echo ""

# ---------------------------------------------------------------------------
# Step 1 — Fetch the currently deployed WASM
# ---------------------------------------------------------------------------
echo "[STEP 1] Fetching currently deployed WASM..."
if ! stellar contract fetch \
    --id "$CONTRACT_ID" \
    --network "$NETWORK" \
    --out-file "$CURRENT_WASM" 2>&1; then
  echo "[ERROR] Failed to fetch deployed WASM for $CONTRACT_NAME ($CONTRACT_ID)"
  echo "        Ensure the Stellar CLI is configured and the network is reachable."
  exit 1
fi
echo "         Saved to $CURRENT_WASM"
echo ""

# ---------------------------------------------------------------------------
# Step 2 — Inspect both WASMs
# ---------------------------------------------------------------------------
echo "[STEP 2] Inspecting WASM specs..."

if ! stellar contract inspect --wasm "$CURRENT_WASM" > "$SPEC_CURRENT" 2>&1; then
  echo "[ERROR] Failed to inspect current WASM. Output:"
  cat "$SPEC_CURRENT"
  exit 1
fi
echo "         Current spec  → $SPEC_CURRENT"

if ! stellar contract inspect --wasm "$NEW_WASM" > "$SPEC_NEW" 2>&1; then
  echo "[ERROR] Failed to inspect new WASM. Output:"
  cat "$SPEC_NEW"
  exit 1
fi
echo "         New spec      → $SPEC_NEW"
echo ""

# ---------------------------------------------------------------------------
# Step 3 — ABI compatibility check (no function removals allowed)
# ---------------------------------------------------------------------------
echo "[STEP 3] Checking ABI compatibility..."

CURRENT_FNS=$(extract_functions "$SPEC_CURRENT")
NEW_FNS=$(extract_functions "$SPEC_NEW")

# Functions present in current but absent from new → breaking removal
REMOVED_FNS=$(comm -23 \
  <(echo "$CURRENT_FNS") \
  <(echo "$NEW_FNS") || true)

# Functions present in new but absent from current → additions (allowed)
ADDED_FNS=$(comm -13 \
  <(echo "$CURRENT_FNS") \
  <(echo "$NEW_FNS") || true)

# Count helpers
count_lines() {
  echo "$1" | grep -c '[^[:space:]]' || echo 0
}

REMOVED_COUNT=$(count_lines "$REMOVED_FNS")
ADDED_COUNT=$(count_lines "$ADDED_FNS")

echo ""
echo "  Functions added   : $ADDED_COUNT"
if [ -n "$ADDED_FNS" ]; then
  while IFS= read -r fn; do
    [ -n "$fn" ] && echo "    + $fn"
  done <<< "$ADDED_FNS"
fi

echo "  Functions removed : $REMOVED_COUNT"
if [ -n "$REMOVED_FNS" ]; then
  while IFS= read -r fn; do
    [ -n "$fn" ] && echo "    - $fn  [BREAKING]"
  done <<< "$REMOVED_FNS"
fi
echo ""

ABI_OK=true
if [ -n "$REMOVED_FNS" ]; then
  echo "[FAIL] ABI BREAKING CHANGE: the following functions were removed:"
  while IFS= read -r fn; do
    [ -n "$fn" ] && echo "         $fn"
  done <<< "$REMOVED_FNS"
  echo ""
  echo "       Removing public contract functions is a breaking change. Clients"
  echo "       that call these functions will fail after the upgrade."
  echo "       Either keep the function (mark it deprecated if desired) or"
  echo "       ensure no callers depend on it before removing."
  ABI_OK=false
fi

# ---------------------------------------------------------------------------
# Step 4 — Storage version check (new must be >= current)
# ---------------------------------------------------------------------------
echo "[STEP 4] Checking STORAGE_VERSION..."

CURRENT_VER=$(extract_storage_version "$SPEC_CURRENT")
NEW_VER=$(extract_storage_version "$SPEC_NEW")

VERSION_OK=true
if [ -z "$CURRENT_VER" ] && [ -z "$NEW_VER" ]; then
  echo "         No STORAGE_VERSION constant found in either spec — skipping version check."
  echo "         (Consider adding 'pub const STORAGE_VERSION: u32 = 1;' to your contract.)"
elif [ -z "$NEW_VER" ] && [ -n "$CURRENT_VER" ]; then
  echo "[FAIL]   STORAGE_VERSION was present in the deployed contract (v${CURRENT_VER})"
  echo "         but is MISSING from the new WASM."
  echo "         Add 'pub const STORAGE_VERSION: u32 = ${CURRENT_VER};' (or higher) to the new contract."
  VERSION_OK=false
elif [ -n "$CURRENT_VER" ] && [ -n "$NEW_VER" ]; then
  echo "         Current STORAGE_VERSION : $CURRENT_VER"
  echo "         New     STORAGE_VERSION : $NEW_VER"
  if [ "$NEW_VER" -lt "$CURRENT_VER" ]; then
    echo "[FAIL]   Storage version DOWNGRADE detected ($CURRENT_VER → $NEW_VER)."
    echo "         The new contract must have a STORAGE_VERSION >= $CURRENT_VER."
    echo "         A downgrade would corrupt on-chain data that was written with the"
    echo "         newer storage layout."
    VERSION_OK=false
  else
    echo "         Version OK ($CURRENT_VER → $NEW_VER)"
  fi
elif [ -z "$CURRENT_VER" ] && [ -n "$NEW_VER" ]; then
  echo "         Current contract has no STORAGE_VERSION; new contract introduces v${NEW_VER}."
  echo "         This is allowed — versioning has been adopted going forward."
fi
echo ""

# ---------------------------------------------------------------------------
# Final verdict
# ---------------------------------------------------------------------------
if [ "$ABI_OK" = false ] || [ "$VERSION_OK" = false ]; then
  echo "========================================================="
  echo " VALIDATION FAILED"
  echo " Upgrade of '$CONTRACT_NAME' on $NETWORK has been REJECTED."
  echo " Fix the issues listed above before retrying."
  echo "========================================================="
  exit 1
fi

echo "========================================================="
echo " VALIDATION PASSED"
echo " '$CONTRACT_NAME' on $NETWORK is safe to upgrade."
echo "========================================================="
exit 0
