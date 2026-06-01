#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
DEPLOYER_ACCOUNT="${DEPLOYER_ACCOUNT:-}"
FUND_AMOUNT="${FUND_AMOUNT:-1000}"

if [[ -z "$DEPLOYER_ACCOUNT" ]]; then
  echo "error: DEPLOYER_ACCOUNT is required" >&2
  exit 2
fi

if ! command -v stellar >/dev/null 2>&1; then
  echo "error: stellar CLI is required" >&2
  exit 127
fi

stellar keys fund "$DEPLOYER_ACCOUNT" --network testnet --amount "$FUND_AMOUNT"
echo "funded $DEPLOYER_ACCOUNT with $FUND_AMOUNT testnet lumens"
