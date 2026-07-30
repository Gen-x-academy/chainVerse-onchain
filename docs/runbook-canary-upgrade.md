# Runbook: Canary Upgrade and Rollback

**Issue:** #803  
**Last updated:** 2026-07-29  
**Applies to:** All contracts in `deployments/testnet.json`

---

## Overview

This runbook covers the safe upgrade path for ChainVerse Soroban contracts using a
canary strategy: one contract instance is upgraded first, validated under real network
conditions for at least five minutes, and only then is the upgrade extended to the
remaining contracts.

Soroban contract upgrades replace the on-chain WASM code by calling the contract's
own `upgrade` function, which is admin-gated. The previous WASM binary is **not**
automatically retained on-chain — you must save the WASM file or its hash before
upgrading. Rollback means re-uploading the previous WASM and calling `upgrade` again
with that hash.

**Critical limitation:** on-chain ledger state (storage entries written by the new
WASM) is NOT automatically reverted on rollback. A rollback only restores the
executing code; data written during the window the new WASM was live persists. If the
new WASM ran a storage migration (e.g., reformatted a key, wrote new mandatory
fields), rolling back the code may leave the contract in a state the old WASM cannot
read correctly. Assess this before every upgrade.

---

## Roles and Responsibilities

### Admin key holder

The deployer account whose keypair was used during `scripts/deploy-testnet.sh` holds
admin authority over all contracts. This identity is stored in the Stellar CLI keystore
under the name `deployer` (or a custom name set via `STELLAR_IDENTITY`).

- Confirm the key name with: `stellar keys ls`
- Confirm the public key with: `stellar keys address deployer`
- The address stored in `deployments/testnet.json` under `"deployer"` must match.

### Multi-sig requirements

The `escrow-vault` contract requires a threshold of approver signatures before any
vault action (including internal admin calls). Confirm the vault's `threshold` value
before upgrading:

```bash
stellar contract invoke \
  --id "$(jq -r '.contracts.escrow_vault' deployments/testnet.json)" \
  --source deployer \
  --network testnet \
  -- get_config
```

For all other contracts, a single admin signature is sufficient for `upgrade`.

### Key rotation

If the admin key needs to be rotated before an upgrade:

1. Generate the new key: `stellar keys generate new-admin --network testnet`
2. Fund the new account via Friendbot:
   ```bash
   curl "https://friendbot.stellar.org?addr=$(stellar keys address new-admin)"
   ```
3. On each contract, call `transfer_admin` (or `set_admin` depending on the
   contract) signed by the **current** admin, passing the new admin address.
4. Update `STELLAR_IDENTITY` in `.env.testnet` and your local shell.
5. Re-confirm access with a read-only call (e.g., `version`) signed by the new key.

---

## Pre-Upgrade Checklist

Complete every item before upgrading any contract. Do not proceed if any item fails.

- [ ] **Save current WASM hash.** Record the SHA-256 of the currently deployed WASM
      for every contract you are about to upgrade. The hash appears in
      `deployments/testnet.json` under `wasm_sha256` if the most recent deploy was
      done with the updated `deploy-testnet.sh`. Verify it matches the file on disk:
      ```bash
      sha256sum contracts/target/wasm32-unknown-unknown/release/<contract>.wasm
      ```
      Write the hash to the rollback ref (see next step).

- [ ] **Write rollback ref.** Create a rollback reference file before touching any
      contract on-chain. The file path convention is:
      ```
      deployments/rollback-refs/<contract_name>-<YYYYMMDD-HHMMSS>.json
      ```
      Minimum required content:
      ```json
      {
        "contract_name": "chv_token",
        "contract_id": "C...",
        "wasm_sha256": "<hex>",
        "wasm_path": "path/to/chv_token.wasm",
        "recorded_at": "2026-07-29T01:00:00Z",
        "recorded_by": "deployer",
        "git_commit": "<short-sha>"
      }
      ```
      `scripts/upgrade-contract.sh` writes this file automatically when it saves the
      rollback ref. The `simulate-rollback.sh` script reads it when performing a
      dry-run or live rollback.

- [ ] **Smoke tests pass on current deployment.**
      ```bash
      ./scripts/smoke-test.sh | tee /tmp/pre-upgrade-smoke.txt
      ```
      All checks must show `PASS` or `SKIP`. Any `FAIL` must be investigated and
      resolved before proceeding.

- [ ] **Deployer account is funded.** Upgrades consume XLM fees.
      ```bash
      stellar keys address deployer | xargs -I{} \
        curl -s "https://horizon-testnet.stellar.org/accounts/{}" \
        | jq '.balances[] | select(.asset_type=="native") | .balance'
      ```
      Ensure at least **5 XLM** is available. Top up if needed:
      ```bash
      curl "https://friendbot.stellar.org?addr=$(stellar keys address deployer)"
      ```

- [ ] **New WASM file exists and is the correct build.**
      ```bash
      ls -lh contracts/target/wasm32-unknown-unknown/release/<contract>.wasm
      # Verify the git commit matches the intended release
      git log --oneline -5
      ```

- [ ] **Identify canary contract.** Choose one contract to upgrade first. Prefer a
      contract with lower blast radius (e.g., `course_registry` or `reward` before
      `escrow` or `chv_token`).

- [ ] **Communicate window.** Notify the team that an upgrade is in progress and
      include the canary contract name and estimated rollout timeline.

---

## Canary Phase

### 1. Upgrade the canary contract

Replace `<canary_contract>` with the chosen contract name (e.g., `course_registry`)
and `<new_wasm>` with the path to the built WASM artifact.

```bash
./scripts/upgrade-contract.sh testnet deployer <canary_contract> \
  contracts/target/wasm32-unknown-unknown/release/<canary_contract>.wasm
```

The script will:
1. Look up the contract ID from `deployments/testnet.json`.
2. Write a rollback ref to `deployments/rollback-refs/`.
3. Call `stellar contract upgrade` with the new WASM.

Record the transaction hash printed in the output. Example output:
```
Upgrading course_registry (CABC...XYZ) to contracts/.../course_registry.wasm on testnet...
Transaction hash: abcd1234ef56...
Upgrade of course_registry completed successfully.
Rollback ref written to deployments/rollback-refs/course_registry-20260729-010300.json
```

### 2. Immediate post-canary health check

Run the smoke tests immediately after the upgrade:

```bash
./scripts/smoke-test.sh | tee /tmp/canary-smoke-immediate.txt
```

### 3. Observe for at least 5 minutes

Wait **at least 5 minutes** without proceeding to the full rollout. During this window:

- Watch for error events or unexpected reverts using Horizon event streaming:
  ```bash
  curl -s "https://horizon-testnet.stellar.org/accounts/$(stellar keys address deployer)/operations?order=desc&limit=10" \
    | jq '[.._embedded.records[]? | {id,type,created_at}]'
  ```
- Check transaction history on the [Stellar Expert testnet explorer](https://stellar.expert/explorer/testnet):
  `https://stellar.expert/explorer/testnet/contract/<contract_id>`

### 4. Health gates — what constitutes healthy

All of the following must be true before proceeding to full rollout:

| Gate | How to verify |
|------|---------------|
| Smoke test passes | `./scripts/smoke-test.sh` exits 0 with no `FAIL` lines |
| Key read functions return expected values | `version`, `is_paused`, `get_backend_pubkey` etc. return values consistent with the pre-upgrade state |
| No auth failures in recent transactions | Horizon operations for the deployer show no `op_bad_auth` or `tx_bad_auth` results |
| No unexpected contract panics | Stellar Expert shows no failed transactions on the contract address in the observation window |
| WASM hash on-chain matches the new artifact | Retrieve the on-chain hash and compare with `sha256sum` of the WASM file |

To retrieve the on-chain WASM hash for verification:
```bash
stellar contract info \
  --id "$(jq -r '.contracts.<contract_name>' deployments/testnet.json)" \
  --network testnet
```

If any gate fails, stop and follow the [Rollback Procedure](#rollback-procedure).

---

## Full Rollout

Proceed only after the canary health gates have all passed and the 5-minute
observation window has elapsed.

Upgrade each remaining contract one at a time. Do not batch upgrades — upgrading
one at a time lets you isolate failures.

```bash
# Upgrade each contract in order of increasing blast radius.
# Adjust the list and order based on which contracts you are releasing.

for CONTRACT in reward certificates course_registry staking payout_automation escrow escrow_vault chv_token chainverse_core; do
  echo "=== Upgrading: $CONTRACT ==="
  ./scripts/upgrade-contract.sh testnet deployer "$CONTRACT" \
    "contracts/target/wasm32-unknown-unknown/release/${CONTRACT}.wasm"

  echo "--- Running smoke test after $CONTRACT upgrade ---"
  ./scripts/smoke-test.sh | tee "/tmp/smoke-after-${CONTRACT}.txt"

  # Check for failures before continuing
  if grep -q "FAIL" "/tmp/smoke-after-${CONTRACT}.txt"; then
    echo "ERROR: smoke test failed after upgrading $CONTRACT. STOPPING rollout."
    echo "Initiate rollback for $CONTRACT before proceeding."
    exit 1
  fi

  echo "--- $CONTRACT healthy. Continuing in 60 seconds ---"
  sleep 60
done
```

Run the final smoke test after all upgrades complete and save the output as the
authoritative post-rollout record:

```bash
./scripts/smoke-test.sh | tee /tmp/post-rollout-smoke.txt
```

---

## Rollback Procedure

### Overview

Rolling back a Soroban contract means re-uploading the previous WASM and calling
`upgrade` with that WASM, which replaces the currently executing code. The contract
ID does not change. **On-chain storage state is NOT reverted** — any data written
by the new WASM while it was live persists on the ledger.

### Step 1: Identify the rollback WASM

The rollback ref files in `deployments/rollback-refs/` record the WASM path and
hash from immediately before each upgrade. Find the correct ref file:

```bash
ls -lt deployments/rollback-refs/<contract_name>-*.json | head -5
```

The most recent file corresponds to the last upgrade. Verify the recorded WASM file
still exists on disk:

```bash
jq -r '.wasm_path' deployments/rollback-refs/<contract_name>-<timestamp>.json
```

If the WASM file no longer exists locally, retrieve it from git using the commit
recorded in the ref:

```bash
COMMIT=$(jq -r '.git_commit' deployments/rollback-refs/<contract_name>-<timestamp>.json)
git show "${COMMIT}:contracts/target/wasm32-unknown-unknown/release/<contract_name>.wasm" \
  > /tmp/<contract_name>-rollback.wasm
```

Note: compiled WASM artifacts are typically not committed to git. If the artifact
is unavailable, rebuild from the recorded commit:

```bash
COMMIT=$(jq -r '.git_commit' deployments/rollback-refs/<contract_name>-<timestamp>.json)
git worktree add /tmp/rollback-build "$COMMIT"
cargo build --manifest-path /tmp/rollback-build/contracts/Cargo.toml \
  --target wasm32-unknown-unknown --release
```

### Step 2: Perform the rollback using simulate-rollback.sh

Use the simulation script to preview and execute the rollback safely:

```bash
./scripts/simulate-rollback.sh testnet <contract_name> <path_to_rollback_wasm>
```

The script will:
1. Find the most recent rollback ref for the contract.
2. Print a dry-run summary (contract ID, timestamp, WASM path, SHA-256).
3. Ask for explicit confirmation before proceeding.
4. Call `upgrade-contract.sh` with the rollback WASM.

### Step 3: Perform rollback manually (if simulate-rollback.sh is unavailable)

```bash
CONTRACT_ID=$(jq -r '.contracts.<contract_name>' deployments/testnet.json)
ROLLBACK_WASM="<path_to_previous_wasm>"

./scripts/upgrade-contract.sh testnet deployer <contract_name> "$ROLLBACK_WASM"
```

Or call the Stellar CLI directly:

```bash
stellar contract upgrade \
  --contract-id "$CONTRACT_ID" \
  --wasm "$ROLLBACK_WASM" \
  --source deployer \
  --network testnet
```

### What state is NOT rolled back

Rolling back the WASM does not affect ledger storage. Specifically:

- Any `put_persistent`, `put_temporary`, or `put_instance` calls made by the new
  WASM while it was live are retained verbatim on the ledger after rollback.
- If the new WASM wrote new storage keys that the old WASM does not know about,
  the old WASM will simply ignore those keys.
- If the new WASM reformatted the encoding of an existing key (e.g., changed a
  struct layout), the old WASM may fail to deserialize the value, causing runtime
  errors on reads.
- If the new WASM deleted a key that the old WASM requires, the old WASM will
  behave as if the key was never set.

After any rollback, run the full smoke test suite and any additional integration
checks to confirm the old code operates correctly against the existing storage state.

### When rollback is not possible

Rollback cannot restore the system to a fully consistent pre-upgrade state in the
following situations:

1. **Storage migration already applied.** If the new WASM ran an explicit migration
   on `upgrade` (e.g., reformatted all existing keys into a new encoding), the old
   WASM will be unable to read those keys correctly after rollback. The only
   resolution is to write a new version of the contract that reads both the old and
   new formats, or to write a migration contract that repairs the storage.

2. **New WASM issued irreversible token transfers or burns.** Token movements are
   independent ledger entries; they cannot be reversed by a contract code rollback.

3. **New WASM emitted events that external systems consumed.** Off-chain indexers
   or frontends may have already acted on events from the upgraded contract.

4. **Ledger entry TTL extended by the new WASM.** TTL changes persist after rollback.

In these cases, document the situation as an incident, determine the safest forward
path (patch release, manual repair, governance vote), and do not rollback if it
would leave the contract in an unreadable state.

---

## Incident Evidence

Capture the following evidence for every failed upgrade or rollback event.

### Transaction hashes

Every `stellar contract upgrade` invocation prints a transaction hash to stdout.
Copy this hash immediately. Example:

```
Transaction hash: 3a8fbc4d9e1a...
```

Record it in your incident notes:

```bash
echo "Contract: chv_token" >> incident-$(date +%Y%m%d).txt
echo "Upgrade tx: 3a8fbc4d9e1a..." >> incident-$(date +%Y%m%d).txt
echo "Rollback tx: b2c7de5f0f2b..." >> incident-$(date +%Y%m%d).txt
```

### Stellar CLI output logs

Redirect all CLI output during the upgrade window to a file:

```bash
./scripts/upgrade-contract.sh testnet deployer chv_token <wasm> 2>&1 \
  | tee /tmp/upgrade-chv_token-$(date +%Y%m%dT%H%M%S).log
```

### Retrieving transaction history on testnet

**Via Horizon REST API:**

```bash
# Recent operations on the deployer account
DEPLOYER=$(stellar keys address deployer)
curl -s "https://horizon-testnet.stellar.org/accounts/${DEPLOYER}/operations?order=desc&limit=20" \
  | jq '[._embedded.records[] | {id, type, created_at, transaction_hash}]'

# Full details of a specific transaction
TX_HASH="<transaction_hash>"
curl -s "https://horizon-testnet.stellar.org/transactions/${TX_HASH}" | jq .

# Operations within a specific transaction
curl -s "https://horizon-testnet.stellar.org/transactions/${TX_HASH}/operations" | jq .
```

**Via Stellar Expert (web):**

- Account history: `https://stellar.expert/explorer/testnet/account/<deployer_address>`
- Transaction detail: `https://stellar.expert/explorer/testnet/tx/<tx_hash>`
- Contract history: `https://stellar.expert/explorer/testnet/contract/<contract_id>`

**Via Stellar CLI:**

```bash
stellar tx fetch <tx_hash> --network testnet
```

### Contract event logs

```bash
CONTRACT_ID=$(jq -r '.contracts.<contract_name>' deployments/testnet.json)
stellar contract events \
  --id "$CONTRACT_ID" \
  --network testnet \
  --start-ledger <ledger_before_upgrade>
```

To find the ledger sequence at a given time, query Horizon:

```bash
curl -s "https://horizon-testnet.stellar.org/ledgers?order=desc&limit=1" \
  | jq '._embedded.records[0] | {sequence, closed_at}'
```

---

## Post-Upgrade Verification

After completing the full rollout (or after a rollback), run the full smoke test
suite and preserve the output.

```bash
# Run smoke tests and capture output with timestamp
TIMESTAMP=$(date +%Y%m%dT%H%M%S)
./scripts/smoke-test.sh 2>&1 | tee "/tmp/post-upgrade-smoke-${TIMESTAMP}.txt"

# Check exit code
echo "Smoke test exit code: $?"
```

Review the output for any `FAIL` lines. A healthy post-upgrade run looks like:

```
=======================================================
 ChainVerse Smoke Test
 Network  : testnet
 Identity : deployer
 File     : deployments/testnet.json
=======================================================
...
 Results: 8 passed  |  0 failed  |  1 skipped
=======================================================
All smoke tests passed!
```

Store the smoke test output alongside the incident record:

```bash
cp "/tmp/post-upgrade-smoke-${TIMESTAMP}.txt" \
   "deployments/incidents/upgrade-${TIMESTAMP}-smoke.txt"
```

Create a summary entry in `deployments/incidents/` (create the directory if needed):

```bash
mkdir -p deployments/incidents
cat > "deployments/incidents/upgrade-${TIMESTAMP}.md" <<EOF
# Upgrade Incident Record — ${TIMESTAMP}

**Operator:** $(stellar keys address deployer)
**Contracts upgraded:** <list>
**Outcome:** success / partial failure / rolled back

## Transaction Hashes
- <contract>: <tx_hash>

## Smoke Test Result
$(cat "/tmp/post-upgrade-smoke-${TIMESTAMP}.txt")
EOF
```

Once the upgrade is confirmed healthy, update `deployments/testnet.json` with the
new WASM hashes (if not already done by `upgrade-contract.sh`) and commit the
updated deployment manifest:

```bash
git add deployments/testnet.json deployments/rollback-refs/
git commit -m "chore(deploy): record upgrade <contract> to <short_commit>"
```
