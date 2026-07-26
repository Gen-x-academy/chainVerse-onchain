#!/usr/bin/env bash
set -euo pipefail

[ -f .env.testnet ] && source .env.testnet

echo "Building all contracts..."
cargo build --target wasm32-unknown-unknown --release

deploy() {
  local name=$1
  local wasm=$2
  echo "Deploying $name..."
  stellar contract deploy \
    --wasm "target/wasm32-unknown-unknown/release/${wasm}.wasm" \
    --source "$STELLAR_IDENTITY" \
    --network testnet
}

CHV_TOKEN_CONTRACT_ID=$(deploy chv_token chv_token)
CERTIFICATES_CONTRACT_ID=$(deploy certificates certificates)
ESCROW_VAULT_CONTRACT_ID=$(deploy escrow-vault escrow_vault)
STAKING_CONTRACT_ID=$(deploy staking staking)
PAYOUT_AUTOMATION_CONTRACT_ID=$(deploy payout-automation payout_automation)
COURSE_REGISTRY_CONTRACT_ID=$(deploy course_registry course_registry)

echo "CHV_TOKEN_CONTRACT_ID=$CHV_TOKEN_CONTRACT_ID"
echo "CERTIFICATES_CONTRACT_ID=$CERTIFICATES_CONTRACT_ID"
echo "ESCROW_VAULT_CONTRACT_ID=$ESCROW_VAULT_CONTRACT_ID"
echo "STAKING_CONTRACT_ID=$STAKING_CONTRACT_ID"
echo "PAYOUT_AUTOMATION_CONTRACT_ID=$PAYOUT_AUTOMATION_CONTRACT_ID"
echo "COURSE_REGISTRY_CONTRACT_ID=$COURSE_REGISTRY_CONTRACT_ID"
