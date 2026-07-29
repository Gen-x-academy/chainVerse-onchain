# Upgrade Validation

ChainVerse contracts are upgradeable via Soroban's `upgrade` host function. Because on-chain storage
is persistent across upgrades, deploying a new WASM that changes the contract's public interface or
storage layout can silently corrupt state or break downstream callers.

`scripts/validate-upgrade.sh` implements a set of pre-upgrade checks that must pass before any
deployment goes through. This document explains the rules the validator enforces and how to use it.

---

## Overview

Before `scripts/upgrade-contract.sh` calls `stellar contract upgrade`, it automatically invokes
`scripts/validate-upgrade.sh` with the same arguments. The validator:

1. Fetches the currently deployed WASM from the network.
2. Runs `stellar contract inspect` on both the deployed and candidate WASMs to extract the
   contract spec (public ABI and constants).
3. Checks that no public function has been removed (ABI compatibility).
4. Checks that the `STORAGE_VERSION` constant has not been decremented (storage compatibility).
5. Exits `0` (VALIDATION PASSED) only if both checks succeed; otherwise exits `1` and aborts the
   upgrade.

After validation, `upgrade-contract.sh` records a rollback reference file in
`deployments/rollback-refs/` containing the new WASM's sha256 hash and the contract ID, so you can
identify what was installed and redeploy a known-good WASM if something goes wrong.

---

## ABI Compatibility Rules

### What is a breaking change?

A **breaking change** is any modification to the contract's public interface that can cause an
existing, correctly-written caller to fail at runtime.

| Change | Classification | Allowed by validator? |
|---|---|---|
| Add a new `pub fn` | Non-breaking | ✅ Yes |
| Add a new optional parameter (Soroban does not support this directly) | Breaking | ❌ No |
| Remove a `pub fn` | Breaking | ❌ No |
| Rename a `pub fn` | Breaking (old name removed) | ❌ No |
| Change a parameter type of an existing `pub fn` | Breaking | ❌ No† |
| Change the return type of an existing `pub fn` | Breaking | ❌ No† |
| Add a new `#[contracttype]` variant (appended) | Non-breaking | ✅ Yes |
| Remove or reorder a `#[contracttype]` variant | Breaking | ❌ No† |

† The validator checks for function *presence* (name-level), not full signature compatibility.
  Signature changes on existing functions are not caught automatically. Teams must review
  parameter and return-type changes manually before merging.

### Deprecation pattern

To retire a function without breaking existing callers, keep the old implementation and mark it
deprecated in the source:

```rust
/// Deprecated: use `transfer_v2` instead. Will be removed in storage v3.
pub fn transfer(env: Env, from: Address, to: Address, amount: i128) -> Result<(), TokenError> {
    // forward to the new implementation
    Self::transfer_v2(env, from, to, amount, None)
}

pub fn transfer_v2(
    env: Env,
    from: Address,
    to: Address,
    amount: i128,
    memo: Option<String>,
) -> Result<(), TokenError> {
    // ...
}
```

Only remove the old function once you are certain no on-chain or off-chain caller references it,
and you have bumped `STORAGE_VERSION` to communicate the layout change.

---

## Storage Versioning Convention

### Defining STORAGE_VERSION

Every deployable contract should declare a `STORAGE_VERSION` constant in its source. The validator
reads this constant from the compiled WASM spec.

```rust
// At the top of lib.rs (or in a dedicated version.rs):

/// Storage layout version. Increment whenever the on-chain data schema changes
/// in a way that requires migration logic or makes old data unreadable.
///
/// Rules:
///   - Bump when adding, removing, or renaming a DataKey variant.
///   - Bump when changing the type stored under an existing DataKey.
///   - Do NOT bump for logic-only changes that leave storage untouched.
pub const STORAGE_VERSION: u32 = 1;
```

The validator parses the `stellar contract inspect` output for this constant. The pattern it matches
is a line containing the token `STORAGE_VERSION` (case-insensitive) followed by an integer.

### When to increment

| Scenario | Increment STORAGE_VERSION? |
|---|---|
| New DataKey added (new feature data) | ✅ Yes |
| DataKey removed | ✅ Yes |
| Value type under an existing key changed | ✅ Yes |
| Logic bug fixed, storage layout unchanged | ❌ No |
| New `pub fn` added that does not touch storage | ❌ No |
| Constants or error codes changed | ❌ No |

### Migration logic

When storage layout changes between versions, add a `migrate` function that transforms old-format
entries into the new format. The upgrade flow is:

```
1. Deploy new WASM via upgrade-contract.sh (validation runs automatically).
2. Call  stellar contract invoke -- migrate  to run the migration.
3. Verify state with smoke-test.sh.
```

Example migration stub:

```rust
/// Migrates storage from version N-1 to STORAGE_VERSION.
/// Must be called once by the admin immediately after upgrading the WASM.
pub fn migrate(env: Env) -> Result<(), ContractError> {
    let admin: Address = env.storage().instance().get(&DataKey::Admin)
        .ok_or(ContractError::NotInitialized)?;
    admin.require_auth();

    let stored_ver: u32 = env.storage().instance()
        .get(&DataKey::StorageVersion)
        .unwrap_or(0);

    if stored_ver >= STORAGE_VERSION {
        return Ok(()); // already migrated
    }

    // --- perform data transformation here ---

    env.storage().instance().set(&DataKey::StorageVersion, &STORAGE_VERSION);
    Ok(())
}
```

---

## How to Run Validation Manually

You can run the validator without performing an actual upgrade to check whether a candidate WASM is
compatible with what is already on-chain:

```sh
./scripts/validate-upgrade.sh <network> <source> <contract-name> <new-wasm-path>
```

**Examples:**

```sh
# Check if a locally built chv_token WASM is safe to deploy on testnet
./scripts/validate-upgrade.sh testnet deployer chv_token \
  ./target/wasm32-unknown-unknown/release/chv_token.wasm

# Check an escrow upgrade on mainnet (dry-run only — upgrade-contract.sh not called)
./scripts/validate-upgrade.sh mainnet admin escrow \
  ./target/wasm32-unknown-unknown/release/escrow.wasm
```

**Output example (passing):**

```
=========================================================
 ChainVerse Upgrade Validator
 Network  : testnet
 Contract : chv_token
 New WASM : ./target/wasm32-unknown-unknown/release/chv_token.wasm
=========================================================

[INFO] Resolved contract ID : CABC...XYZ

[STEP 1] Fetching currently deployed WASM...
         Saved to /tmp/current_contract_chv_token.wasm

[STEP 2] Inspecting WASM specs...
         Current spec  → /tmp/spec_current_chv_token.txt
         New spec      → /tmp/spec_new_chv_token.txt

[STEP 3] Checking ABI compatibility...

  Functions added   : 1
    + transfer_v2
  Functions removed : 0

[STEP 4] Checking STORAGE_VERSION...
         Current STORAGE_VERSION : 2
         New     STORAGE_VERSION : 3
         Version OK (2 → 3)

=========================================================
 VALIDATION PASSED
 'chv_token' on testnet is safe to upgrade.
=========================================================
```

**Output example (failing — removed function):**

```
[STEP 3] Checking ABI compatibility...

  Functions added   : 0
  Functions removed : 1
    - freeze_account  [BREAKING]

[FAIL] ABI BREAKING CHANGE: the following functions were removed:
         freeze_account

       Removing public contract functions is a breaking change. Clients
       that call these functions will fail after the upgrade.
```

### Inspecting specs without a live network

You can inspect any WASM file locally without fetching from the network:

```sh
stellar contract inspect --wasm ./target/wasm32-unknown-unknown/release/chv_token.wasm
```

This prints the full contract spec including all public functions, types, and constants, which is
useful for manual review before committing a change.

---

## CI Integration Notes

### Automatic validation in upgrade-contract.sh

`scripts/upgrade-contract.sh` already calls `validate-upgrade.sh` automatically. If validation
fails the script exits with code `1` and the `stellar contract upgrade` call is never reached, so
the on-chain contract is not touched.

### Adding a validation job to GitHub Actions

To catch breaking changes at PR time — before anyone runs an actual upgrade — add a workflow step
that runs the validator against the testnet deployment:

```yaml
# .github/workflows/validate-upgrade.yml
name: Validate upgrade compatibility

on:
  pull_request:
    paths:
      - 'contracts/**'

jobs:
  validate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Stellar CLI
        run: cargo install --locked stellar-cli@22.0.0 --features opt

      - name: Build WASMs
        run: stellar contract build

      - name: Validate chv_token upgrade
        env:
          STELLAR_NETWORK: testnet
        run: |
          ./scripts/validate-upgrade.sh testnet ci chv_token \
            ./target/wasm32-unknown-unknown/release/chv_token.wasm
        # This step will fail the PR if a public function was removed or
        # STORAGE_VERSION was decremented relative to the testnet deployment.

      - name: Validate escrow upgrade
        run: |
          ./scripts/validate-upgrade.sh testnet ci escrow \
            ./target/wasm32-unknown-unknown/release/escrow.wasm
```

**Notes for CI:**

- The CI identity (`ci` above) only needs read access to the network for `stellar contract fetch`.
  It does not need signing keys for actual upgrades.
- If `deployments/testnet.json` contains empty contract IDs (contracts not yet deployed),
  `validate-upgrade.sh` will exit with an error. Gate the validation step on the existence of a
  deployed contract ID, or skip it for contracts that have never been deployed:

  ```yaml
  - name: Validate escrow upgrade (skip if not deployed)
    run: |
      id=$(python3 -c "import json; d=json.load(open('deployments/testnet.json')); print(d['contracts'].get('escrow',''))")
      if [ -n "$id" ]; then
        ./scripts/validate-upgrade.sh testnet ci escrow \
          ./target/wasm32-unknown-unknown/release/escrow.wasm
      else
        echo "Skipping: escrow not yet deployed on testnet"
      fi
  ```

- Rollback reference files written to `deployments/rollback-refs/` are local artifacts. Add that
  directory to `.gitignore` unless you want them committed:

  ```gitignore
  deployments/rollback-refs/
  ```

  Or commit them to preserve an audit trail of every upgrade that was performed.
