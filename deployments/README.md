# Deployments

This directory contains deployment metadata for ChainVerse smart contracts on Stellar networks.

## Testnet Contracts

| Contract           | Address | Description                                      |
|--------------------|---------|--------------------------------------------------|
| escrow             | TBD     | Buyer-seller payment escrow with token transfer  |
| escrow_vault       | TBD     | Multisig vault requiring approver threshold      |
| certificates       | TBD     | Soulbound course completion NFT certificates     |
| chainverse_core    | TBD     | Core platform logic and access control           |
| reward             | TBD     | On-chain reward distribution for learners        |
| chv_token          | TBD     | CHV ERC-20-style token (fixed supply, 7 decimals)|
| course_registry    | TBD     | Registry of courses and enrollment records       |
| payout_automation  | TBD     | Automated instructor payout processing           |

> Addresses are populated automatically in [`testnet.json`](./testnet.json) by `scripts/deploy-testnet.sh` after each deployment.

## Network Details

| Field      | Value                                      |
|------------|--------------------------------------------|
| Network    | Testnet                                    |
| RPC URL    | https://soroban-testnet.stellar.org        |
| Passphrase | `Test SDF Network ; September 2015`        |

## Interacting with Contracts

Use the [Soroban CLI](https://soroban.stellar.org/docs/getting-started/setup):

```bash
soroban contract invoke \
  --id <CONTRACT_ADDRESS> \
  --source <YOUR_KEY> \
  --network testnet \
  -- <FUNCTION_NAME> [ARGS]
```
