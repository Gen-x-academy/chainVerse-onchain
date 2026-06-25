# Deployment Guide

## Prerequisites

Before the CI deploy workflow can run, repo admins must add the following secrets under **Settings → Secrets and variables → Actions**.

### Required GitHub Secrets

| Secret | Value |
|---|---|
| `STELLAR_SECRET_KEY` | Testnet deployer secret key (funded via Friendbot) |
| `STELLAR_NETWORK` | `testnet` |
| `STELLAR_RPC_URL` | `https://soroban-testnet.stellar.org` |

### Generating and Funding a Deployer Account

1. Generate a new keypair:
   ```bash
   stellar keys generate deployer --network testnet
   stellar keys address deployer      # copy the public key
   stellar keys show deployer         # copy the secret key → use as STELLAR_SECRET_KEY
   ```

2. Fund the account via Friendbot:
   ```bash
   curl "https://friendbot.stellar.org?addr=<PUBLIC_KEY>"
   ```

   Or run the helper script:
   ```bash
   ./scripts/setup-testnet-account.sh
   ```

### Adding Secrets to GitHub

1. Go to your repository on GitHub.
2. Navigate to **Settings → Secrets and variables → Actions**.
3. Click **New repository secret** for each of the three secrets above.

## Deploy Workflow

Once secrets are configured, build and deploy all contracts to testnet:

```bash
# Build all contracts
./scripts/build-all.sh

# Deploy to testnet (writes addresses to deployments/testnet.json)
./scripts/deploy-testnet.sh
```

Deployed contract addresses are recorded in `deployments/testnet.json` and can be referenced by the frontend and backend.

## Initializing Contracts After Deployment

After deploying, each contract's `initialize()` function must be called to set the admin and initial configuration. Run the initialization script:

```bash
# Initialize all deployed contracts (reads from deployments/testnet.json)
./scripts/initialize-contracts.sh
```

The script reads contract addresses from `deployments/testnet.json` and calls the appropriate initialization function on each contract. Extra configuration is supplied via environment variables:

| Variable | Default | Description |
|---|---|---|
| `ADMIN_ADDRESS` | Derived from `deployer` key | Admin address set on all contracts |
| `STELLAR_SOURCE` | `deployer` | Stellar CLI key name used to sign transactions |
| `BACKEND_PUBLIC_KEY` | — | Hex-encoded public key for certificate proof verification (required for `certificates` contract) |
| `TREASURY_ADDRESS` | Same as `ADMIN_ADDRESS` | Treasury address for the `reward` contract |
| `CHV_TOKEN_ADDRESS` | Read from `deployments/testnet.json` | CHV token contract address (used by `reward` and `chainverse_core`) |
| `PROTOCOL_FEE_BPS` | `100` | Protocol fee in basis points for `chainverse_core` (100 = 1%) |
| `REWARD_AMOUNT` | `10000000` | Per-claim reward amount (stroops) for the `reward` contract |

Example with all variables set:

```bash
ADMIN_ADDRESS="GABC...XYZ" \
BACKEND_PUBLIC_KEY="deadbeef..." \
TREASURY_ADDRESS="GABC...XYZ" \
PROTOCOL_FEE_BPS=100 \
REWARD_AMOUNT=10000000 \
./scripts/initialize-contracts.sh
```

## Network Details

| Property | Value |
|---|---|
| Network | `testnet` |
| RPC URL | `https://soroban-testnet.stellar.org` |
| Passphrase | `Test SDF Network ; September 2015` |
