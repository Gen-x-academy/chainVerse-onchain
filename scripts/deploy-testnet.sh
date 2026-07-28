#!/usr/bin/env bash
# scripts/deploy-testnet.sh
#
# Deploy all ChainVerse contracts to Stellar testnet and write
# contract addresses to deployments/testnet.json and .env.testnet.
# Deploy all ChainVerse contracts to Stellar testnet and atomically write
# contract addresses to deployments/testnet.json and .env.testnet.
# Deploy all ChainVerse contracts to Stellar testnet. Records WASM hashes,
# source revision, SDK version, and deployer in the deployment manifest.
#
# Usage:
#   cp .env.testnet.example .env.testnet   # set STELLAR_IDENTITY etc.
#   ./scripts/deploy-testnet.sh
set -euo pipefail

NETWORK="${STELLAR_NETWORK:-testnet}"
STELLAR_IDENTITY="${STELLAR_IDENTITY:-deployer}"
CONTRACTS_DIR="${CONTRACTS_DIR:-contracts}"

# ---------------------------------------------------------------------------
# Pre-flight: validate working directory
# ---------------------------------------------------------------------------
if [ ! -d "$CONTRACTS_DIR" ]; then
  echo "ERROR: contracts directory not found: $CONTRACTS_DIR" >&2
  exit 1
fi

# Pre-flight: validate network passphrase
# ---------------------------------------------------------------------------
EXPECTED_PASSPHRASE="Test SDF Network ; September 2015"
if [ "$NETWORK" = "testnet" ]; then
  ACTUAL_PASSPHRASE=$(stellar network passphrase --network testnet 2>/dev/null || echo "")
  if [ -n "$ACTUAL_PASSPHRASE" ] && [ "$ACTUAL_PASSPHRASE" != "$EXPECTED_PASSPHRASE" ]; then
    echo "ERROR: network passphrase mismatch" >&2
    echo "  expected: $EXPECTED_PASSPHRASE" >&2
    echo "  actual:   $ACTUAL_PASSPHRASE" >&2
    exit 1
  fi
fi

# ---------------------------------------------------------------------------
# Source metadata
# ---------------------------------------------------------------------------
SOURCE_COMMIT=$(git rev-parse HEAD 2>/dev/null || echo "unknown")
SOURCE_COMMIT_SHORT=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
SDK_VERSION=$(grep 'soroban-sdk' "$CONTRACTS_DIR/Cargo.toml" | head -1 | grep -oP '[\d.]+' || echo "unknown")
DEPLOYER_ADDRESS=$(stellar keys address "$STELLAR_IDENTITY" 2>/dev/null || echo "unknown")

echo "Source commit: $SOURCE_COMMIT_SHORT"
echo "SDK version:   $SDK_VERSION"
echo "Deployer:      $DEPLOYER_ADDRESS"
echo ""

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------
if [ ! -d "$CONTRACTS_DIR" ]; then
  echo "ERROR: contracts directory not found: $CONTRACTS_DIR" >&2
  exit 1
fi

echo "Building all contracts in $CONTRACTS_DIR..."
cargo build --manifest-path "$CONTRACTS_DIR/Cargo.toml" \
  --target wasm32-unknown-unknown --release

# ---------------------------------------------------------------------------
# Helper: deploy a single WASM and return its contract ID
# ---------------------------------------------------------------------------
deploy() {
  local crate_name=$1
  local wasm_name=${2:-$crate_name}
  local wasm_path="$CONTRACTS_DIR/target/wasm32-unknown-unknown/release/${wasm_name}.wasm"

  if [ ! -f "$wasm_path" ]; then
    echo "ERROR: WASM artifact not found: $wasm_path" >&2
    exit 1
  fi

  echo "Deploying $crate_name..."
  stellar contract deploy \
    --wasm "$wasm_path" \
    --source "$STELLAR_IDENTITY" \
    --network "$NETWORK"
}

# ---------------------------------------------------------------------------
# Deploy all platform contracts (order matters for cross-contract refs)
# ---------------------------------------------------------------------------
CHV_TOKEN_CONTRACT_ID=$(deploy chv_token chv_token)
CERTIFICATES_CONTRACT_ID=$(deploy certificates certificates)
ESCROW_CONTRACT_ID=$(deploy escrow escrow)
ESCROW_VAULT_CONTRACT_ID=$(deploy escrow-vault escrow_vault)
CHAINVERSE_CORE_CONTRACT_ID=$(deploy chainverse-core chainverse_core)
REWARD_CONTRACT_ID=$(deploy reward reward)
STAKING_CONTRACT_ID=$(deploy staking staking)
PAYOUT_AUTOMATION_CONTRACT_ID=$(deploy payout-automation payout_automation)
COURSE_REGISTRY_CONTRACT_ID=$(deploy course_registry course_registry)

# ---------------------------------------------------------------------------
# Atomic write: deployment manifest (JSON)
  local wasm_sha
  wasm_sha=$(sha256sum "$wasm_path" | cut -d' ' -f1)

  echo "Deploying $crate_name (sha256: ${wasm_sha:0:16}...)..."
  local contract_id
  contract_id=$(stellar contract deploy \
    --wasm "$wasm_path" \
    --source "$STELLAR_IDENTITY" \
    --network "$NETWORK")

  echo "$contract_id|$wasm_sha"
}

# ---------------------------------------------------------------------------
# Deploy all platform contracts
# ---------------------------------------------------------------------------
deploy_result() { echo "$1" | cut -d'|' -f1; }
deploy_sha()    { echo "$1" | cut -d'|' -f2; }

R_CHV=$(deploy chv_token chv_token)
CHV_TOKEN_CONTRACT_ID=$(deploy_result "$R_CHV")
CHV_SHA=$(deploy_sha "$R_CHV")

R_CERT=$(deploy certificates certificates)
CERTIFICATES_CONTRACT_ID=$(deploy_result "$R_CERT")
CERT_SHA=$(deploy_sha "$R_CERT")

R_ESCROW=$(deploy escrow escrow)
ESCROW_CONTRACT_ID=$(deploy_result "$R_ESCROW")
ESCROW_SHA=$(deploy_sha "$R_ESCROW")

R_VAULT=$(deploy escrow-vault escrow_vault)
ESCROW_VAULT_CONTRACT_ID=$(deploy_result "$R_VAULT")
VAULT_SHA=$(deploy_sha "$R_VAULT")

R_CORE=$(deploy chainverse-core chainverse_core)
CHAINVERSE_CORE_CONTRACT_ID=$(deploy_result "$R_CORE")
CORE_SHA=$(deploy_sha "$R_CORE")

R_REWARD=$(deploy reward reward)
REWARD_CONTRACT_ID=$(deploy_result "$R_REWARD")
REWARD_SHA=$(deploy_sha "$R_REWARD")

R_STAKING=$(deploy staking staking)
STAKING_CONTRACT_ID=$(deploy_result "$R_STAKING")
STAKING_SHA=$(deploy_sha "$R_STAKING")

R_PAYOUT=$(deploy payout-automation payout_automation)
PAYOUT_AUTOMATION_CONTRACT_ID=$(deploy_result "$R_PAYOUT")
PAYOUT_SHA=$(deploy_sha "$R_PAYOUT")

R_COURSE=$(deploy course_registry course_registry)
COURSE_REGISTRY_CONTRACT_ID=$(deploy_result "$R_COURSE")
COURSE_SHA=$(deploy_sha "$R_COURSE")

# ---------------------------------------------------------------------------
# Atomic write: deployment manifest (JSON) with WASM hashes and source info
# ---------------------------------------------------------------------------
mkdir -p "$(dirname "deployments/testnet.json")"
TEMP_JSON="deployments/testnet.json.tmp.$$"
cat > "$TEMP_JSON" <<EOJSON
{
  "network": "$NETWORK",
  "rpc_url": "https://soroban-testnet.stellar.org",
  "passphrase": "Test SDF Network ; September 2015",
  "deployed_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "deployer": "$STELLAR_IDENTITY",
  "contracts": {
    "chv_token": "$CHV_TOKEN_CONTRACT_ID",
    "certificates": "$CERTIFICATES_CONTRACT_ID",
    "escrow": "$ESCROW_CONTRACT_ID",
    "escrow_vault": "$ESCROW_VAULT_CONTRACT_ID",
    "chainverse_core": "$CHAINVERSE_CORE_CONTRACT_ID",
    "reward": "$REWARD_CONTRACT_ID",
    "staking": "$STAKING_CONTRACT_ID",
    "payout_automation": "$PAYOUT_AUTOMATION_CONTRACT_ID",
    "course_registry": "$COURSE_REGISTRY_CONTRACT_ID"
  "passphrase": "Test SDF Network ; September 2015",
  "rpc_url": "https://soroban-testnet.stellar.org",
  "deployed_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "deployer": "$DEPLOYER_ADDRESS",
  "source_commit": "$SOURCE_COMMIT",
  "sdk_version": "$SDK_VERSION",
  "contracts": {
    "chv_token":        { "address": "$CHV_TOKEN_CONTRACT_ID",    "wasm_sha256": "$CHV_SHA" },
    "certificates":     { "address": "$CERTIFICATES_CONTRACT_ID", "wasm_sha256": "$CERT_SHA" },
    "escrow":           { "address": "$ESCROW_CONTRACT_ID",       "wasm_sha256": "$ESCROW_SHA" },
    "escrow_vault":     { "address": "$ESCROW_VAULT_CONTRACT_ID", "wasm_sha256": "$VAULT_SHA" },
    "chainverse_core":  { "address": "$CHAINVERSE_CORE_CONTRACT_ID","wasm_sha256": "$CORE_SHA" },
    "reward":           { "address": "$REWARD_CONTRACT_ID",       "wasm_sha256": "$REWARD_SHA" },
    "staking":          { "address": "$STAKING_CONTRACT_ID",      "wasm_sha256": "$STAKING_SHA" },
    "payout_automation":{ "address": "$PAYOUT_AUTOMATION_CONTRACT_ID","wasm_sha256": "$PAYOUT_SHA" },
    "course_registry":  { "address": "$COURSE_REGISTRY_CONTRACT_ID","wasm_sha256": "$COURSE_SHA" }
  }
}
EOJSON
mv "$TEMP_JSON" "deployments/testnet.json"
echo "Deployment manifest written to deployments/testnet.json"

# ---------------------------------------------------------------------------
# Atomic write: environment file
# ---------------------------------------------------------------------------
TEMP_ENV=".env.testnet.tmp.$$"
cat > "$TEMP_ENV" <<EOENV
# Auto-generated by scripts/deploy-testnet.sh — do not edit manually.
STELLAR_NETWORK=$NETWORK
STELLAR_IDENTITY=$STELLAR_IDENTITY
CHV_TOKEN_CONTRACT_ID=$CHV_TOKEN_CONTRACT_ID
CERTIFICATES_CONTRACT_ID=$CERTIFICATES_CONTRACT_ID
ESCROW_CONTRACT_ID=$ESCROW_CONTRACT_ID
ESCROW_VAULT_CONTRACT_ID=$ESCROW_VAULT_CONTRACT_ID
CHAINVERSE_CORE_CONTRACT_ID=$CHAINVERSE_CORE_CONTRACT_ID
REWARD_CONTRACT_ID=$REWARD_CONTRACT_ID
STAKING_CONTRACT_ID=$STAKING_CONTRACT_ID
PAYOUT_AUTOMATION_CONTRACT_ID=$PAYOUT_AUTOMATION_CONTRACT_ID
COURSE_REGISTRY_CONTRACT_ID=$COURSE_REGISTRY_CONTRACT_ID
EOENV
mv "$TEMP_ENV" ".env.testnet"
echo "Environment written to .env.testnet"

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo ""
echo "=== Deployment complete ==="
echo "Source:     $SOURCE_COMMIT_SHORT"
echo "SDK:        $SDK_VERSION"
echo "Deployer:   $DEPLOYER_ADDRESS"
echo ""
echo "CHV Token:            $CHV_TOKEN_CONTRACT_ID"
echo "Certificates:         $CERTIFICATES_CONTRACT_ID"
echo "Escrow:               $ESCROW_CONTRACT_ID"
echo "Escrow Vault:         $ESCROW_VAULT_CONTRACT_ID"
echo "ChainVerse Core:      $CHAINVERSE_CORE_CONTRACT_ID"
echo "Reward:               $REWARD_CONTRACT_ID"
echo "Staking:              $STAKING_CONTRACT_ID"
echo "Payout Automation:    $PAYOUT_AUTOMATION_CONTRACT_ID"
echo "Course Registry:      $COURSE_REGISTRY_CONTRACT_ID"
