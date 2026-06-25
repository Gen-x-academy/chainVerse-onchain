#!/bin/bash
set -e

DEPLOYMENT_FILE="${1:-deployments/testnet.json}"
NETWORK="testnet"
PASS=0
FAIL=0

if [ ! -f "$DEPLOYMENT_FILE" ]; then
  echo "ERROR: $DEPLOYMENT_FILE not found"
  exit 1
fi

# Read contract addresses
get_address() {
  grep -o "\"$1\": \"[^\"]*\"" "$DEPLOYMENT_FILE" | grep -o '"[^"]*"$' | tr -d '"'
}

check_contract() {
  local name="$1"
  local fn="$2"
  local id
  id=$(get_address "$name")

  if [ -z "$id" ] || [ "$id" = "ERROR" ]; then
    echo "  SKIP $name (no address)"
    return
  fi

  echo -n "  Checking $name ($id)... "
  if stellar contract invoke \
      --id "$id" \
      --network "$NETWORK" \
      --source deployer \
      -- "$fn" >/dev/null 2>&1; then
    echo "OK"
    PASS=$((PASS + 1))
  else
    echo "FAIL"
    FAIL=$((FAIL + 1))
  fi
}

echo "=== Smoke Test: $DEPLOYMENT_FILE ==="
check_contract "escrow"            "get_escrow_count"
check_contract "escrow_vault"      "get_admin"
check_contract "certificates"      "get_admin"
check_contract "chainverse_core"   "get_admin"
check_contract "reward"            "get_admin"
check_contract "chv_token"         "get_admin"
check_contract "course_registry"   "get_admin"
check_contract "payout_automation" "get_admin"

echo ""
echo "Results: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
