# Testnet Setup Guide

This guide takes a new contributor from zero to a full set of ChainVerse contracts
running on Stellar testnet — identity, funding, build, deploy, initialize, and
smoke test.

For a deeper walkthrough of just the identity/funding step, see
[testnet-identity-setup.md](testnet-identity-setup.md).

## 1. Install Stellar CLI

```sh
cargo install --locked stellar-cli --features opt
```

Verify the installation:

```sh
stellar --version
```

## 2. Create a deployer identity

```sh
stellar keys generate deployer --network testnet
```

## 3. Get your public key

```sh
stellar keys address deployer
```

## 4. Fund via Friendbot

```sh
curl "https://friendbot.stellar.org?addr=$(stellar keys address deployer)"
```

## 5. Verify balance

```sh
stellar account show deployer --network testnet
```

You should see a balance of 10,000 XLM.

## 6. Build contracts

```sh
cargo build --target wasm32-unknown-unknown --release
```

Compiled WASM files are output to `target/wasm32-unknown-unknown/release/`.

## 7. Deploy

```sh
chmod +x scripts/deploy-testnet.sh
STELLAR_IDENTITY=deployer ./scripts/deploy-testnet.sh
```

This prints a `<CONTRACT_NAME>_CONTRACT_ID=...` line for each deployed
contract. Copy `.env.testnet.example` to `.env.testnet` and fill in the
printed contract IDs:

```sh
cp .env.testnet.example .env.testnet
```

## 8. Initialize

```sh
chmod +x scripts/init-contracts.sh
./scripts/init-contracts.sh
```

This seeds admin, token, and treasury addresses on each deployed contract.
It is safe to re-run — already-initialized contracts report their
`AlreadyInitialized` error and the script continues.

Note: `init-contracts.sh` requires `CERTIFICATES_BACKEND_PUBKEY_HEX` to be
set in `.env.testnet` (see the comment above it in `.env.testnet.example`
for how to generate one).

## 9. Smoke test

```sh
chmod +x scripts/smoke-test.sh
./scripts/smoke-test.sh
```

This invokes a read-only function on each deployed contract and reports
pass/fail/skip counts. It exits non-zero if any check fails.

## Done

At this point you should have a full set of ChainVerse contracts deployed,
initialized, and verified live on Stellar testnet — in well under 30 minutes.
