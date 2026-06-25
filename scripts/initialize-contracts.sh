#!/bin/bash
# Calls initialize() (or set_admin) on each deployed contract listed in
# deployments/testnet.json.  Run this once after deploy-testnet.sh completes.
set -euo pipefail

NETWORK="${STELLAR_NETWORK:-testnet}"
SOURCE="${STELLAR_SOURCE:-deployer}"
DEPLOYMENT_FILE="${DEPLOYMENT_FILE:-deployments/testnet.json}"

if [ ! -f "$DEPLOYMENT_FILE" ]; then
  echo "error: $DEPLOYMENT_FILE not found. Run ./scripts/deploy-testnet.sh first." >&2
  exit 1
fi

if ! command -v jq &>/dev/null; then
  echo "error: jq is required. Install it with: apt-get install jq" >&2
  exit 1
fi

# Resolve admin address from env or from the configured Stellar key
if [ -z "${ADMIN_ADDRESS:-}" ]; then
  ADMIN_ADDRESS=$(stellar keys address "$SOURCE" 2>/dev/null || true)
fi
if [ -z "${ADMIN_ADDRESS:-}" ]; then
  echo "error: could not determine ADMIN_ADDRESS." >&2
  echo "  Either set ADMIN_ADDRESS env var or configure the '$SOURCE' Stellar key." >&2
  exit 1
fi

echo "Network:        $NETWORK"
echo "Source key:     $SOURCE"
echo "Admin address:  $ADMIN_ADDRESS"
echo ""

# Read a contract ID from the deployment file (exits 0 even when value is empty)
get_id() {
  jq -r ".contracts.$1 // empty" "$DEPLOYMENT_FILE"
}

# Invoke a contract function; logs the call and exits on failure
invoke() {
  local contract_id="$1"
  local fn="$2"
  shift 2
  echo "  invoking $fn $*"
  stellar contract invoke \
    --id    "$contract_id" \
    --source "$SOURCE" \
    --network "$NETWORK" \
    -- "$fn" "$@"
}

# Skip a contract when its address is missing or empty
check_id() {
  local name="$1"
  local id="$2"
  if [ -z "$id" ] || [ "$id" = "null" ] || [ "$id" = "ERROR" ]; then
    echo "  SKIP — no valid contract ID for '$name'"
    return 1
  fi
  return 0
}

# ---------------------------------------------------------------------------
# chv_token  — initialize first; its address is used by reward & core
# ---------------------------------------------------------------------------
CHV_TOKEN_ID=$(get_id "chv_token")
echo "[chv_token]  $CHV_TOKEN_ID"
if check_id "chv_token" "$CHV_TOKEN_ID"; then
  invoke "$CHV_TOKEN_ID" initialize \
    --admin "$ADMIN_ADDRESS"
fi

# ---------------------------------------------------------------------------
# course_registry
# ---------------------------------------------------------------------------
COURSE_REGISTRY_ID=$(get_id "course_registry")
echo "[course_registry]  $COURSE_REGISTRY_ID"
if check_id "course_registry" "$COURSE_REGISTRY_ID"; then
  invoke "$COURSE_REGISTRY_ID" initialize \
    --admin "$ADMIN_ADDRESS"
fi

# ---------------------------------------------------------------------------
# payout_automation
# ---------------------------------------------------------------------------
PAYOUT_ID=$(get_id "payout_automation")
echo "[payout_automation]  $PAYOUT_ID"
if check_id "payout_automation" "$PAYOUT_ID"; then
  invoke "$PAYOUT_ID" initialize \
    --admin "$ADMIN_ADDRESS"
fi

# ---------------------------------------------------------------------------
# escrow  — uses set_admin instead of initialize
# ---------------------------------------------------------------------------
ESCROW_ID=$(get_id "escrow")
echo "[escrow]  $ESCROW_ID"
if check_id "escrow" "$ESCROW_ID"; then
  invoke "$ESCROW_ID" set_admin \
    --admin "$ADMIN_ADDRESS"
fi

# ---------------------------------------------------------------------------
# escrow_vault  — uses set_admin instead of initialize
# ---------------------------------------------------------------------------
ESCROW_VAULT_ID=$(get_id "escrow_vault")
echo "[escrow_vault]  $ESCROW_VAULT_ID"
if check_id "escrow_vault" "$ESCROW_VAULT_ID"; then
  invoke "$ESCROW_VAULT_ID" set_admin \
    --admin "$ADMIN_ADDRESS"
fi

# ---------------------------------------------------------------------------
# certificates  — requires a backend public key for proof verification
# ---------------------------------------------------------------------------
CERT_ID=$(get_id "certificates")
echo "[certificates]  $CERT_ID"
if check_id "certificates" "$CERT_ID"; then
  if [ -z "${BACKEND_PUBLIC_KEY:-}" ]; then
    echo "  SKIP — BACKEND_PUBLIC_KEY env var is required for certificates contract"
  else
    invoke "$CERT_ID" init \
      --admin "$ADMIN_ADDRESS" \
      --backend_public_key "$BACKEND_PUBLIC_KEY"
  fi
fi

# ---------------------------------------------------------------------------
# reward  — requires treasury, token, and reward_amount
# ---------------------------------------------------------------------------
REWARD_ID=$(get_id "reward")
echo "[reward]  $REWARD_ID"
if check_id "reward" "$REWARD_ID"; then
  TREASURY_ADDRESS="${TREASURY_ADDRESS:-$ADMIN_ADDRESS}"
  # Default to the deployed chv_token if not overridden
  CHV_TOKEN_FOR_REWARD="${CHV_TOKEN_ADDRESS:-$CHV_TOKEN_ID}"
  REWARD_AMOUNT="${REWARD_AMOUNT:-10000000}"

  if [ -z "$CHV_TOKEN_FOR_REWARD" ] || [ "$CHV_TOKEN_FOR_REWARD" = "null" ]; then
    echo "  SKIP — CHV_TOKEN_ADDRESS is required for reward contract (chv_token not deployed)"
  else
    invoke "$REWARD_ID" initialize \
      --admin "$ADMIN_ADDRESS" \
      --treasury "$TREASURY_ADDRESS" \
      --token "$CHV_TOKEN_FOR_REWARD" \
      --reward_amount "$REWARD_AMOUNT"
  fi
fi

# ---------------------------------------------------------------------------
# chainverse_core  — requires protocol_fee and supported_tokens list
# ---------------------------------------------------------------------------
CORE_ID=$(get_id "chainverse_core")
echo "[chainverse_core]  $CORE_ID"
if check_id "chainverse_core" "$CORE_ID"; then
  PROTOCOL_FEE_BPS="${PROTOCOL_FEE_BPS:-100}"
  CHV_TOKEN_FOR_CORE="${CHV_TOKEN_ADDRESS:-$CHV_TOKEN_ID}"

  if [ -z "$CHV_TOKEN_FOR_CORE" ] || [ "$CHV_TOKEN_FOR_CORE" = "null" ]; then
    TOKENS_JSON="[]"
  else
    TOKENS_JSON="[\"$CHV_TOKEN_FOR_CORE\"]"
  fi

  invoke "$CORE_ID" initialize \
    --admin "$ADMIN_ADDRESS" \
    --protocol_fee "$PROTOCOL_FEE_BPS" \
    --supported_tokens "$TOKENS_JSON"
fi

echo ""
echo "Initialization complete."
