#!/usr/bin/env bash
# scripts/preflight.sh
#
# Check that all required toolchains and CLI tools are installed and
# compatible before building or deploying contracts.
#
# Usage:
#   ./scripts/preflight.sh
#
# Exit codes:
#   0 — all checks passed
#   1 — one or more checks failed

set -euo pipefail

PASS=0
FAIL=0

check() {
  local name="$1"
  shift
  local output
  if output=$("$@" 2>&1); then
    echo "  [OK]   $name"
    PASS=$((PASS + 1))
  else
    echo "  [FAIL] $name — $output"
    FAIL=$((FAIL + 1))
  fi
}

check_version() {
  local name="$1"
  local cmd="$2"
  local expected="$3"
  local actual
  if actual=$("$cmd" --version 2>&1 | head -1); then
    if echo "$actual" | grep -q "$expected"; then
      echo "  [OK]   $name ($actual)"
      PASS=$((PASS + 1))
    else
      echo "  [FAIL] $name — expected $expected, got $actual"
      FAIL=$((FAIL + 1))
    fi
  else
    echo "  [FAIL] $name — not found or --version failed"
    FAIL=$((FAIL + 1))
  fi
}

echo "======================================================="
echo " ChainVerse Preflight Check"
echo "======================================================="
echo ""

# --- Rust ---
echo "--- Rust Toolchain ---"
check_version "rustc" "rustc" "1.85"
check_version "cargo" "cargo" ""

# --- wasm32 target ---
echo ""
echo "--- wasm32 Target ---"
if rustup target list --installed 2>/dev/null | grep -q wasm32-unknown-unknown; then
  echo "  [OK]   wasm32-unknown-unknown target installed"
  PASS=$((PASS + 1))
else
  echo "  [FAIL] wasm32-unknown-unknown target — run: rustup target add wasm32-unknown-unknown"
  FAIL=$((FAIL + 1))
fi

# --- Stellar CLI ---
echo ""
echo "--- Stellar CLI ---"
check_version "stellar" "stellar" "22"

# --- Network ---
echo ""
echo "--- Network Profile ---"
NETWORK="${STELLAR_NETWORK:-testnet}"
IDENTITY="${STELLAR_IDENTITY:-deployer}"
echo "  [INFO] Network:  $NETWORK"
echo "  [INFO] Identity: $IDENTITY"

if stellar keys address "$IDENTITY" >/dev/null 2>&1; then
  echo "  [OK]   Identity '$IDENTITY' configured"
  PASS=$((PASS + 1))
else
  echo "  [WARN] Identity '$IDENTITY' not found — run: stellar keys generate --global $IDENTITY --network testnet --fund"
fi

# --- Shell tools ---
echo ""
echo "--- Shell Tools ---"
for tool in git sha256sum jq; do
  if command -v "$tool" >/dev/null 2>&1; then
    echo "  [OK]   $tool"
    PASS=$((PASS + 1))
  else
    echo "  [WARN] $tool — not found (optional)"
  fi
done

# --- Contracts workspace ---
echo ""
echo "--- Contracts Workspace ---"
if [ -d "contracts" ] && [ -f "contracts/Cargo.toml" ]; then
  echo "  [OK]   contracts/ directory found"
  PASS=$((PASS + 1))
else
  echo "  [FAIL] contracts/ directory or Cargo.toml not found"
  FAIL=$((FAIL + 1))
fi

# --- Summary ---
echo ""
echo "======================================================="
echo " Results: ${PASS} passed  |  ${FAIL} failed"
echo "======================================================="

if [ "$FAIL" -gt 0 ]; then
  echo "Preflight check failed — fix the issues above before building."
  exit 1
fi

echo "All preflight checks passed!"
exit 0
