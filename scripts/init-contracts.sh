#!/usr/bin/env bash
set -uo pipefail

source .env.testnet

invoke() {
  local contract_id=$1
  local name=$2
  shift 2
  if stellar contract invoke --id "$contract_id" --source "$STELLAR_IDENTITY" --network testnet -- "$@"; then
    echo "✓ $name initialized"
  else
    echo "✗ $name initialization failed"
  fi
}

invoke "$CHV_TOKEN_CONTRACT_ID"        "chv_token"          initialize --admin "$STELLAR_IDENTITY" --decimal 7 --name "ChainVerse Token" --symbol "CHV"
invoke "$CERTIFICATES_CONTRACT_ID"     "certificates"       initialize --admin "$STELLAR_IDENTITY"
invoke "$ESCROW_VAULT_CONTRACT_ID"     "escrow-vault"       initialize --admin "$STELLAR_IDENTITY" --token "$CHV_TOKEN_CONTRACT_ID"
invoke "$STAKING_CONTRACT_ID"          "staking"            initialize --admin "$STELLAR_IDENTITY" --token "$CHV_TOKEN_CONTRACT_ID"
invoke "$PAYOUT_AUTOMATION_CONTRACT_ID" "payout-automation" initialize --admin "$STELLAR_IDENTITY" --token "$CHV_TOKEN_CONTRACT_ID"
invoke "$COURSE_REGISTRY_CONTRACT_ID"  "course_registry"    initialize --admin "$STELLAR_IDENTITY"
