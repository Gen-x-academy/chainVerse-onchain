#!/bin/bash
set -e

NETWORK="${1:-testnet}"
SOURCE="${2:-deployer}"
CONTRACT_NAME="$3"
NEW_WASM="$4"

if [ -z "$CONTRACT_NAME" ] || [ -z "$NEW_WASM" ]; then
  echo "Usage: $0 [network] [source] <contract-name> <new-wasm-path>"
  echo ""
  echo "Examples:"
  echo "  $0 testnet deployer chv_token ./contracts/target/wasm32-unknown-unknown/release/chv_token.wasm"
  echo "  $0 mainnet admin escrow ./contracts/target/wasm32-unknown-unknown/release/escrow.wasm"
  exit 1
fi

RPC_URL="https://soroban-${NETWORK}.stellar.org"
PASSPHRASE="Test SDF Network ; September 2015"
if [ "$NETWORK" = "mainnet" ]; then
  PASSPHRASE="Public Global Stellar Network ; September 2015"
  RPC_URL="https://soroban-rpc.mainnet.stellar.org"
fi

DEPLOYMENTS_FILE="deployments/${NETWORK}.json"

if [ ! -f "$DEPLOYMENTS_FILE" ]; then
  echo "Error: $DEPLOYMENTS_FILE not found. Run deploy-testnet.sh first."
  exit 1
fi

CONTRACT_ID=$(python3 -c "
import json
with open('$DEPLOYMENTS_FILE') as f:
    data = json.load(f)
print(data.get('contracts', {}).get('$CONTRACT_NAME', ''))
")

if [ -z "$CONTRACT_ID" ]; then
  echo "Error: Contract '$CONTRACT_NAME' not found in $DEPLOYMENTS_FILE"
  exit 1
fi

echo "Upgrading $CONTRACT_NAME ($CONTRACT_ID) to $NEW_WASM on $NETWORK..."

if [ ! -f "$NEW_WASM" ]; then
  echo "Error: WASM file $NEW_WASM not found"
  exit 1
fi

stellar contract upgrade \
  --contract-id "$CONTRACT_ID" \
  --wasm "$NEW_WASM" \
  --source "$SOURCE" \
  --network "$NETWORK"

echo "Upgrade of $CONTRACT_NAME completed successfully."
