# ChainVerse Testnet Deployment Guide

## Prerequisites

- **Rust** with the `wasm32-unknown-unknown` target:
  ```bash
  rustup target add wasm32-unknown-unknown
  ```
- **Stellar CLI** installed:
  ```bash
  cargo install --locked stellar-cli --features opt
  ```
- A **funded testnet account** (see [Account Setup](#account-setup) below).

## Account Setup

Generate a deployer keypair and fund it via Friendbot:

```bash
./scripts/setup-testnet-account.sh
```

Or manually:

```bash
stellar keys generate deployer --network testnet
PUBKEY=$(stellar keys address deployer)
curl -s "https://friendbot.stellar.org?addr=$PUBKEY"
```

Keep the secret key from `stellar keys show deployer` — you will need it as `STELLAR_SECRET_KEY`.

## Environment Variable Setup

| Variable | Description |
|---|---|
| `STELLAR_SECRET_KEY` | Secret key of the funded deployer account |
| `STELLAR_NETWORK` | `testnet` |
| `STELLAR_RPC_URL` | `https://soroban-testnet.stellar.org` |

Export them locally before running scripts:

```bash
export STELLAR_SECRET_KEY=<your-secret-key>
export STELLAR_NETWORK=testnet
export STELLAR_RPC_URL=https://soroban-testnet.stellar.org
```

## Manual Deploy Process

1. **Top up the deployer account** (pre-flight Friendbot call):
   ```bash
   ./scripts/fund-testnet.sh
   ```

2. **Build all contracts** to WASM:
   ```bash
   ./scripts/build-all.sh
   ```

3. **Deploy all contracts** to testnet:
   ```bash
   ./scripts/deploy-testnet.sh
   ```

   Contract addresses are written to `deployments/testnet.json`.

## Automated CI Deploy Process

The GitHub Actions workflow (`.github/workflows/ci.yml`) runs on every push to `main`. It:

1. Installs Rust and the Stellar CLI (cached between runs).
2. Sets up the `deployer` identity from the `STELLAR_SECRET_KEY` repository secret.
3. Builds all contracts.
4. Runs `cargo test`.

To trigger a deploy from CI, ensure the following repository secrets are set under **Settings → Secrets and variables → Actions**:

| Secret | Value |
|---|---|
| `STELLAR_SECRET_KEY` | Testnet deployer secret key |
| `STELLAR_NETWORK` | `testnet` |
| `STELLAR_RPC_URL` | `https://soroban-testnet.stellar.org` |

## Verifying Deployment

After deploying, call `version()` on each contract to confirm it is live:

```bash
stellar contract invoke \
  --id <CONTRACT_ADDRESS> \
  --network testnet \
  -- version
```

Expected output: `1` (incremented on each breaking change).

You can also run the smoke-test script once it is in place:

```bash
./scripts/smoke-test.sh
```

## Reading Contract Addresses

Deployed addresses are stored in `deployments/testnet.json`:

```json
{
  "network": "testnet",
  "deployed_at": "2024-01-01T00:00:00Z",
  "contracts": {
    "escrow": "C...",
    "escrow_vault": "C...",
    "certificates": "C...",
    "chainverse_core": "C...",
    "reward": "C...",
    "chv_token": "C...",
    "course_registry": "C...",
    "payout_automation": "C..."
  }
}
```

Read a specific address:

```bash
jq -r '.contracts.escrow' deployments/testnet.json
```

## Resetting Testnet State

Testnet state is ephemeral — accounts and contracts can expire. To reset:

1. Re-fund the deployer:
   ```bash
   PUBKEY=$(stellar keys address deployer --network testnet)
   curl -s "https://friendbot.stellar.org?addr=$PUBKEY"
   ```

2. Re-deploy all contracts:
   ```bash
   ./scripts/build-all.sh
   ./scripts/deploy-testnet.sh
   ```

   `deployments/testnet.json` will be overwritten with the new addresses.

3. Update any frontend or backend configuration that references the old contract addresses.

## Network Details

| Property | Value |
|---|---|
| Network | `testnet` |
| RPC URL | `https://soroban-testnet.stellar.org` |
| Passphrase | `Test SDF Network ; September 2015` |
