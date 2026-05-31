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

## Network Details

| Property | Value |
|---|---|
| Network | `testnet` |
| RPC URL | `https://soroban-testnet.stellar.org` |
| Passphrase | `Test SDF Network ; September 2015` |
