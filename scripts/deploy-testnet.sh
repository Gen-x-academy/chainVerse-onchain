#!/bin/bash
set -e

NETWORK="testnet"
SOURCE="deployer"
RPC_URL="https://soroban-testnet.stellar.org"
PASSPHRASE="Test SDF Network ; September 2015"
WASM_DIR="contracts/target/wasm32-unknown-unknown/release"
OUTPUT_DIR="deployments"
OUTPUT_FILE="$OUTPUT_DIR/testnet.json"

# Ensure WASMs exist
if [ ! -d "$WASM_DIR" ]; then
  echo "No WASM build found. Run ./scripts/build-all.sh first."
  exit 1
fi

WASMS=$(find "$WASM_DIR" -maxdepth 1 -name "*.wasm" | sort)
if [ -z "$WASMS" ]; then
  echo "No .wasm files found in $WASM_DIR. Run ./scripts/build-all.sh first."
  exit 1
fi

mkdir -p "$OUTPUT_DIR"

# Start JSON output
echo "{" > "$OUTPUT_FILE"
echo "  \"network\": \"$NETWORK\"," >> "$OUTPUT_FILE"
echo "  \"rpc_url\": \"$RPC_URL\"," >> "$OUTPUT_FILE"
echo "  \"passphrase\": \"$PASSPHRASE\"," >> "$OUTPUT_FILE"
echo "  \"deployed_at\": \"$(date -u +"%Y-%m-%dT%H:%M:%SZ")\"," >> "$OUTPUT_FILE"
echo "  \"contracts\": {" >> "$OUTPUT_FILE"

FIRST=true
for WASM in $WASMS; do
  NAME=$(basename "$WASM" .wasm)

  echo "Deploying $NAME..."

  CONTRACT_ID=$(stellar contract deploy \
    --wasm "$WASM" \
    --source "$SOURCE" \
    --network "$NETWORK" 2>&1)

  if [ $? -ne 0 ]; then
    echo "  ERROR deploying $NAME: $CONTRACT_ID"
    CONTRACT_ID="ERROR"
  else
    echo "  $NAME => $CONTRACT_ID"
  fi

  if [ "$FIRST" = true ]; then
    FIRST=false
  else
    # Close previous entry
    sed -i '$ s/$/,/' "$OUTPUT_FILE"
  fi

  echo "    \"$NAME\": \"$CONTRACT_ID\"" >> "$OUTPUT_FILE"
done

echo "  }" >> "$OUTPUT_FILE"
echo "}" >> "$OUTPUT_FILE"

echo ""
echo "Deployment complete. Addresses written to $OUTPUT_FILE"
cat "$OUTPUT_FILE"
