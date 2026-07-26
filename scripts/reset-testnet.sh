#!/bin/bash
set -e

DEPLOYMENT_FILE="deployments/testnet.json"
NETWORK="testnet"
SOURCE="deployer"

echo "=== Reset Testnet ==="

# 1. Clear deployments/testnet.json
cat > "$DEPLOYMENT_FILE" <<'EOF'
{
  "network": "testnet",
  "rpc_url": "https://soroban-testnet.stellar.org",
  "passphrase": "Test SDF Network ; September 2015",
  "deployed_at": "",
  "contracts": {
    "escrow": "",
    "escrow_vault": "",
    "certificates": "",
    "chainverse_core": "",
    "reward": "",
    "chv_token": "",
    "course_registry": "",
    "payout_automation": ""
  }
}
EOF
echo "Cleared $DEPLOYMENT_FILE"

# 2. Re-create or reuse deployer account
if stellar keys show "$SOURCE" >/dev/null 2>&1; then
  echo "Reusing existing '$SOURCE' identity"
else
  echo "Generating new '$SOURCE' identity"
  stellar keys generate "$SOURCE" --network "$NETWORK"
  stellar keys fund "$SOURCE" --network "$NETWORK"
fi

# 3. Build
echo "Building contracts..."
(cd contracts && make build)

# 4. Deploy
echo "Deploying contracts..."
bash scripts/deploy-testnet.sh

# 5. Smoke test
echo "Running smoke tests..."
bash scripts/smoke-test.sh

echo ""
echo "=== Testnet reset complete ==="
