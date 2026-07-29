#!/usr/bin/env bash
# scripts/simulate-rollback.sh
#
# Simulate (dry-run) or execute a rollback for a deployed ChainVerse contract
# by re-uploading a previous WASM via scripts/upgrade-contract.sh.
#
# Usage:
#   ./scripts/simulate-rollback.sh <NETWORK> <CONTRACT_NAME> <ROLLBACK_WASM_PATH>
#
# Arguments:
#   NETWORK            — Stellar network name: testnet | mainnet
#   CONTRACT_NAME      — Contract key as it appears in deployments/<NETWORK>.json
#                        e.g. chv_token, escrow, certificates, chainverse_core
#   ROLLBACK_WASM_PATH — Path to the WASM file to roll back to.
#                        If omitted, the script reads the path from the most recent
#                        rollback ref in deployments/rollback-refs/.
#
# The script reads the most recent rollback ref file from
# deployments/rollback-refs/<CONTRACT_NAME>-*.json, prints a dry-run summary,
# asks for confirmation, then calls scripts/upgrade-contract.sh.
#
# Rollback ref files are created automatically by scripts/upgrade-contract.sh
# each time a contract is upgraded. They record the contract ID, WASM hash,
# WASM path, and timestamp from immediately before the upgrade.
#
# Exit codes:
#   0 — rollback executed successfully (or user aborted)
#   1 — bad arguments, missing files, or upgrade failure

set -euo pipefail

# ---------------------------------------------------------------------------
# Arguments
# ---------------------------------------------------------------------------
NETWORK="${1:-}"
CONTRACT_NAME="${2:-}"
ROLLBACK_WASM_PATH="${3:-}"

if [ -z "$NETWORK" ] || [ -z "$CONTRACT_NAME" ]; then
  echo "Usage: $0 <NETWORK> <CONTRACT_NAME> [ROLLBACK_WASM_PATH]" >&2
  echo "" >&2
  echo "  NETWORK            testnet | mainnet" >&2
  echo "  CONTRACT_NAME      e.g. chv_token, escrow, certificates" >&2
  echo "  ROLLBACK_WASM_PATH path to the rollback WASM (optional if ref file has it)" >&2
  echo "" >&2
  echo "Examples:" >&2
  echo "  $0 testnet chv_token ./contracts/target/wasm32-unknown-unknown/release/chv_token.wasm" >&2
  echo "  $0 testnet escrow   # uses path recorded in most recent rollback ref" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

REFS_DIR="$REPO_ROOT/deployments/rollback-refs"
DEPLOYMENTS_FILE="$REPO_ROOT/deployments/${NETWORK}.json"
UPGRADE_SCRIPT="$SCRIPT_DIR/upgrade-contract.sh"

# ---------------------------------------------------------------------------
# Validate prerequisites
# ---------------------------------------------------------------------------
if [ ! -f "$DEPLOYMENTS_FILE" ]; then
  echo "ERROR: deployments file not found: $DEPLOYMENTS_FILE" >&2
  echo "  Have you deployed to $NETWORK yet? Run scripts/deploy-testnet.sh first." >&2
  exit 1
fi

if [ ! -f "$UPGRADE_SCRIPT" ]; then
  echo "ERROR: upgrade script not found: $UPGRADE_SCRIPT" >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# Find the most recent rollback ref for this contract
# ---------------------------------------------------------------------------
REF_FILE=""
if [ -d "$REFS_DIR" ]; then
  # Sort by filename descending (timestamps are embedded: <name>-YYYYMMDD-HHMMSS.json)
  REF_FILE=$(ls -1 "$REFS_DIR/${CONTRACT_NAME}-"*.json 2>/dev/null \
    | sort -r \
    | head -1 || true)
fi

if [ -z "$REF_FILE" ]; then
  echo "WARNING: no rollback ref found in $REFS_DIR for contract '$CONTRACT_NAME'." >&2
  if [ -z "$ROLLBACK_WASM_PATH" ]; then
    echo "ERROR: no rollback ref and no ROLLBACK_WASM_PATH provided." >&2
    echo "  Either supply a WASM path as the third argument, or ensure a rollback ref" >&2
    echo "  exists under deployments/rollback-refs/${CONTRACT_NAME}-*.json" >&2
    exit 1
  fi
  REF_CONTRACT_ID=""
  REF_WASM_SHA256=""
  REF_RECORDED_AT=""
  REF_GIT_COMMIT=""
else
  echo "Using rollback ref: $REF_FILE"
  # Parse ref fields (use python3 for reliable JSON parsing; fall back to grep)
  if command -v python3 &>/dev/null; then
    REF_CONTRACT_ID=$(python3 -c "import json,sys; d=json.load(open('$REF_FILE')); print(d.get('contract_id',''))")
    REF_WASM_SHA256=$(python3 -c "import json,sys; d=json.load(open('$REF_FILE')); print(d.get('wasm_sha256',''))")
    REF_RECORDED_AT=$(python3 -c "import json,sys; d=json.load(open('$REF_FILE')); print(d.get('recorded_at',''))")
    REF_GIT_COMMIT=$(python3 -c  "import json,sys; d=json.load(open('$REF_FILE')); print(d.get('git_commit',''))")
    REF_WASM_PATH=$(python3 -c   "import json,sys; d=json.load(open('$REF_FILE')); print(d.get('wasm_path',''))")
  else
    REF_CONTRACT_ID=$(grep -o '"contract_id"[[:space:]]*:[[:space:]]*"[^"]*"' "$REF_FILE" | cut -d'"' -f4 || true)
    REF_WASM_SHA256=$(grep -o '"wasm_sha256"[[:space:]]*:[[:space:]]*"[^"]*"' "$REF_FILE" | cut -d'"' -f4 || true)
    REF_RECORDED_AT=$(grep -o '"recorded_at"[[:space:]]*:[[:space:]]*"[^"]*"' "$REF_FILE" | cut -d'"' -f4 || true)
    REF_GIT_COMMIT=$(grep  -o '"git_commit"[[:space:]]*:[[:space:]]*"[^"]*"'  "$REF_FILE" | cut -d'"' -f4 || true)
    REF_WASM_PATH=$(grep   -o '"wasm_path"[[:space:]]*:[[:space:]]*"[^"]*"'   "$REF_FILE" | cut -d'"' -f4 || true)
  fi

  # If no explicit WASM path was given, use the one recorded in the ref
  if [ -z "$ROLLBACK_WASM_PATH" ] && [ -n "$REF_WASM_PATH" ]; then
    ROLLBACK_WASM_PATH="$REF_WASM_PATH"
  fi
fi

# ---------------------------------------------------------------------------
# Resolve contract ID from deployments JSON (source of truth for live state)
# ---------------------------------------------------------------------------
LIVE_CONTRACT_ID=""
if command -v python3 &>/dev/null; then
  LIVE_CONTRACT_ID=$(python3 -c "
import json
with open('$DEPLOYMENTS_FILE') as f:
    data = json.load(f)
contracts = data.get('contracts', {})
entry = contracts.get('$CONTRACT_NAME', '')
if isinstance(entry, dict):
    print(entry.get('address', ''))
else:
    print(entry)
")
else
  LIVE_CONTRACT_ID=$(grep -o "\"${CONTRACT_NAME}\"[[:space:]]*:[[:space:]]*\"C[A-Z0-9]*\"" "$DEPLOYMENTS_FILE" \
    | grep -o '"C[A-Z0-9]*"' | tr -d '"' || true)
fi

if [ -z "$LIVE_CONTRACT_ID" ]; then
  echo "ERROR: contract '$CONTRACT_NAME' not found in $DEPLOYMENTS_FILE" >&2
  exit 1
fi

# Use contract ID from ref if live deployments file does not have it yet
CONTRACT_ID="${LIVE_CONTRACT_ID:-$REF_CONTRACT_ID}"

# ---------------------------------------------------------------------------
# Validate WASM path
# ---------------------------------------------------------------------------
if [ -z "$ROLLBACK_WASM_PATH" ]; then
  echo "ERROR: ROLLBACK_WASM_PATH could not be determined." >&2
  echo "  Supply it as the third argument or ensure the rollback ref records 'wasm_path'." >&2
  exit 1
fi

if [ ! -f "$ROLLBACK_WASM_PATH" ]; then
  echo "ERROR: rollback WASM file not found: $ROLLBACK_WASM_PATH" >&2
  echo "" >&2
  echo "  The file may need to be rebuilt from the commit recorded in the rollback ref." >&2
  if [ -n "${REF_GIT_COMMIT:-}" ]; then
    echo "  Recorded commit: $REF_GIT_COMMIT" >&2
    echo "" >&2
    echo "  To rebuild from that commit:" >&2
    echo "    git worktree add /tmp/rollback-build $REF_GIT_COMMIT" >&2
    echo "    cargo build --manifest-path /tmp/rollback-build/contracts/Cargo.toml \\" >&2
    echo "      --target wasm32-unknown-unknown --release" >&2
  fi
  exit 1
fi

ROLLBACK_WASM_SHA256=$(sha256sum "$ROLLBACK_WASM_PATH" | cut -d' ' -f1)
NOW=$(date -u +%Y-%m-%dT%H:%M:%SZ)

# ---------------------------------------------------------------------------
# Dry-run summary
# ---------------------------------------------------------------------------
echo ""
echo "╔══════════════════════════════════════════════════════════╗"
echo "║           ROLLBACK DRY-RUN SUMMARY                      ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""
echo "  Network:          $NETWORK"
echo "  Contract name:    $CONTRACT_NAME"
echo "  Contract ID:      $CONTRACT_ID"
echo "  Timestamp:        $NOW"
echo ""
echo "  Rollback WASM:    $ROLLBACK_WASM_PATH"
echo "  WASM SHA-256:     $ROLLBACK_WASM_SHA256"
if [ -n "${REF_WASM_SHA256:-}" ]; then
  if [ "$ROLLBACK_WASM_SHA256" = "$REF_WASM_SHA256" ]; then
    echo "  Hash matches ref: YES ✓"
  else
    echo "  Hash matches ref: NO — ref recorded $REF_WASM_SHA256"
    echo "                    CAUTION: the WASM on disk differs from the ref." >&2
  fi
fi
echo ""
if [ -n "${REF_RECORDED_AT:-}" ]; then
  echo "  Rollback ref file:     $REF_FILE"
  echo "  Ref recorded at:       $REF_RECORDED_AT"
  if [ -n "${REF_GIT_COMMIT:-}" ]; then
    echo "  Ref git commit:        $REF_GIT_COMMIT"
  fi
fi
echo ""
echo "  ┌─ IMPORTANT REMINDER ─────────────────────────────────┐"
echo "  │ On-chain storage written by the current WASM will    │"
echo "  │ NOT be reverted. Only the executing code changes.    │"
echo "  │ Verify the old WASM can safely read existing state   │"
echo "  │ before confirming.                                   │"
echo "  └──────────────────────────────────────────────────────┘"
echo ""

# ---------------------------------------------------------------------------
# Confirmation prompt
# ---------------------------------------------------------------------------
read -r -p "Proceed with rollback? Type 'yes' to confirm: " CONFIRM

if [ "$CONFIRM" != "yes" ]; then
  echo ""
  echo "Rollback aborted by operator."
  exit 0
fi

echo ""
echo "Executing rollback of $CONTRACT_NAME on $NETWORK..."
echo ""

# ---------------------------------------------------------------------------
# Execute rollback via upgrade-contract.sh
# ---------------------------------------------------------------------------
"$UPGRADE_SCRIPT" "$NETWORK" deployer "$CONTRACT_NAME" "$ROLLBACK_WASM_PATH"

echo ""
echo "Rollback of $CONTRACT_NAME completed."
echo "Run the smoke tests to verify the contract is healthy:"
echo ""
echo "  ./scripts/smoke-test.sh | tee /tmp/post-rollback-smoke-\$(date +%Y%m%dT%H%M%S).txt"
echo ""
