# Stellar Testnet Identity Setup Guide

This guide walks you through creating a funded Stellar testnet identity so you can deploy and interact with ChainVerse contracts on the Stellar testnet.

## Step 1 — Install Stellar CLI

```sh
cargo install --locked stellar-cli
```

Verify the installation:
```sh
stellar --version
```

## Step 2 — Create an Identity

Generate a new keypair and store it locally under a name you choose (e.g. `alice`):

```sh
stellar keys generate alice
```

Display the public key:
```sh
stellar keys address alice
```

> The private key is stored in `~/.config/soroban/identity/alice.toml`. Keep this file secure and never commit it.

## Step 3 — Fund via Friendbot

Friendbot is the Stellar testnet faucet. Fund your new address with 10,000 test XLM:

```sh
stellar friendbot --network testnet alice
```

Alternatively, call Friendbot directly:
```sh
curl "https://friendbot.stellar.org?addr=$(stellar keys address alice)"
```

## Step 4 — Verify Balance

Confirm the account is funded:
```sh
stellar account show alice --network testnet
```

You should see a balance of 10,000 XLM in the output.

## Step 5 — Set as Default Signer for Deployments

Pass `--source alice` to any `stellar contract deploy` or `stellar contract invoke` command to use this identity as the transaction signer:

```sh
stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/chv_token.wasm \
  --source alice \
  --network testnet
```

To avoid repeating `--source` every time, configure the default network and signer in your project:

```sh
stellar network add testnet \
  --rpc-url https://soroban-testnet.stellar.org \
  --network-passphrase "Test SDF Network ; September 2015"
```

Then invoke contracts without specifying `--network` each time:

```sh
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source alice \
  --network testnet \
  -- <function> <args>
```
