#!/bin/bash
set -e

NETWORK="testnet"
DEPLOYMENT_FILE="${1:-deployments/testnet.json}"
PASS=0
FAIL=0

if [ ! -f "$DEPLOYMENT_FILE" ]; then
  echo "ERROR: Deployment file $DEPLOYMENT_FILE not found"
  exit 1
fi

get_address() {
  grep -o "\"$1\": \"[^\"]*\"" "$DEPLOYMENT_FILE" | grep -o '"[^"]*"$' | tr -d '"'
}

check_contract() {
  local name="$1"
  local id
  id=$(get_address "$name")

  if [ -z "$id" ] || [ "$id" = "ERROR" ]; then
    echo "  SKIP $name (no address)"
    return
  fi

  echo -n "  Checking $name ($id)... "
  if stellar contract id --wasm hash "$id" &>/dev/null; then
    echo "OK"
    ((PASS++))
  else
    echo "FAIL"
    ((FAIL++))
  fi
}

echo "========================================"
echo "  SMALDA Testnet Health Check"
echo "========================================"
echo ""

for contract in certificates chv_token escrow escrow-vault payout-automation reward course_registry; do
  check_contract "$contract"
done

echo ""
echo "========================================"
echo "  Results: $PASS passed, $FAIL failed"
echo "========================================"

exit $FAIL
