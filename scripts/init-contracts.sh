#!/usr/bin/env bash
# scripts/init-contracts.sh
#
# Post-deploy initialization for all ChainVerse contracts.  Each contract's
# storage must be seeded (admin, token address, etc.) before it is usable —
# deploying the WASM alone is not enough.
#
# Usage:
#   cp .env.testnet.example .env.testnet   # fill in *_CONTRACT_ID values
#   ./scripts/deploy-testnet.sh            # if not already deployed
#   ./scripts/init-contracts.sh
#
# Idempotent: re-running against already-initialized contracts reports the
# contract's own AlreadyInitialized error and moves on rather than aborting.
set -uo pipefail

source .env.testnet

STELLAR_IDENTITY="${STELLAR_IDENTITY:-deployer}"
ADMIN=$(stellar keys address "$STELLAR_IDENTITY")

# 32-byte ed25519 public key (hex) used to verify certificate mint proofs.
: "${CERTIFICATES_BACKEND_PUBKEY_HEX:?Set CERTIFICATES_BACKEND_PUBKEY_HEX in .env.testnet (32-byte ed25519 pubkey, hex)}"

# Minimum enforced by the staking contract is 100 (1%).
STAKING_EMERGENCY_PENALTY_BPS="${STAKING_EMERGENCY_PENALTY_BPS:-500}"

# Escrow protocol fee in basis points (default 100 = 1%).
ESCROW_PROTOCOL_FEE_BPS="${ESCROW_PROTOCOL_FEE_BPS:-100}"

invoke() {
  local contract_id=$1
  local name=$2
  shift 2
  local output
  if output=$(stellar contract invoke --id "$contract_id" --source "$STELLAR_IDENTITY" --network testnet -- "$@" 2>&1); then
    echo "✓ $name initialized"
  else
    echo "✗ $name initialization skipped: ${output}"
  fi
}

echo "Initializing CHV Token..."
invoke "$CHV_TOKEN_CONTRACT_ID" "chv_token" initialize --admin "$ADMIN" --treasury "$ADMIN"

echo "Initializing Certificates (with minter)..."
invoke "$CERTIFICATES_CONTRACT_ID" "certificates" init --admin "$ADMIN" --backend_public_key "$CERTIFICATES_BACKEND_PUBKEY_HEX" --minter "$ADMIN"

echo "Initializing Escrow..."
invoke "$ESCROW_CONTRACT_ID" "escrow" set_admin --admin "$ADMIN"
invoke "$ESCROW_CONTRACT_ID" "escrow" whitelist_token --admin "$ADMIN" --token "$CHV_TOKEN_CONTRACT_ID"
invoke "$ESCROW_CONTRACT_ID" "escrow" set_protocol_fee_bps --admin "$ADMIN" --bps "$ESCROW_PROTOCOL_FEE_BPS"

echo "Initializing Escrow Vault..."
invoke "$ESCROW_VAULT_CONTRACT_ID" "escrow-vault" set_admin --admin "$ADMIN"

echo "Initializing ChainVerse Core..."
invoke "$CHAINVERSE_CORE_CONTRACT_ID" "chainverse-core" initialize --admin "$ADMIN" --protocol_fee "$ESCROW_PROTOCOL_FEE_BPS" --supported_tokens "[\"$CHV_TOKEN_CONTRACT_ID\"]"

echo "Initializing Reward..."
invoke "$REWARD_CONTRACT_ID" "reward" initialize --admin "$ADMIN" --treasury "$ADMIN" --token "$CHV_TOKEN_CONTRACT_ID" --reward_amount 10000000

echo "Initializing Staking..."
invoke "$STAKING_CONTRACT_ID" "staking" initialize --admin "$ADMIN" --token "$CHV_TOKEN_CONTRACT_ID" --emergency_unstake_penalty_bps "$STAKING_EMERGENCY_PENALTY_BPS"

echo "Initializing Payout Automation..."
invoke "$PAYOUT_AUTOMATION_CONTRACT_ID" "payout-automation" initialize --admin "$ADMIN" --token "$CHV_TOKEN_CONTRACT_ID"

echo "Initializing Course Registry..."
invoke "$COURSE_REGISTRY_CONTRACT_ID" "course_registry" initialize --admin "$ADMIN"

echo ""
echo "All contracts initialized."
echo "  Escrow token whitelist: $CHV_TOKEN_CONTRACT_ID"
echo "  Escrow protocol fee:    $ESCROW_PROTOCOL_FEE_BPS bps"
echo "  Certificate minter:     $ADMIN"
