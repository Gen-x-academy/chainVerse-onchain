# chainVerse-onchain

Smart contracts for the ChainVerse Academy platform, built with [Soroban](https://soroban.stellar.org/) on the Stellar network.

## Project Overview

ChainVerse Academy is a decentralized learning platform where course creators publish content and learners earn verifiable credentials. This repository contains the on-chain contracts that power token incentives, course enrollment, certification, staking, escrow, and automated payouts.

## Contracts

| Contract | Path | Description |
|---|---|---|
| `chv_token` | `contracts/chv_token` | Platform utility token (CHV) — transfer, balances, admin upgrade |
| `certificates` | `contracts/certificates` | Issue and verify on-chain course completion certificates |
| `escrow-vault` | `contracts/escrow-vault` | Holds learner payments until course milestones are met |
| `staking` | `contracts/staking` | Stake CHV tokens to earn rewards and access gated content |
| `payout-automation` | `contracts/payout-automation` | Automatically distribute creator revenue on completion events |
| `course_registry` | `contracts/course_registry` | Register, update, and query available courses |

## Prerequisites

- **Rust** (stable) — [install](https://rustup.rs/)
  ```sh
  rustup target add wasm32-unknown-unknown
  ```
- **Soroban CLI** — [install](https://soroban.stellar.org/docs/getting-started/setup)
  ```sh
  cargo install --locked soroban-cli
  ```
- **Stellar CLI** (optional, for identity and testnet management)
  ```sh
  cargo install --locked stellar-cli
  ```

## Build

Build all contracts:
```sh
cargo build --target wasm32-unknown-unknown --release
```

Build a single contract:
```sh
cargo build --target wasm32-unknown-unknown --release -p chv_token
```

Compiled WASM files are output to `target/wasm32-unknown-unknown/release/`.

## Test

Run the full test suite:
```sh
cargo test
```

Run tests for a specific contract:
```sh
cargo test -p chv_token
```

## Deployment Overview

1. Set up a funded testnet identity (see [docs/testnet-identity-setup.md](docs/testnet-identity-setup.md)).
2. Deploy a contract:
   ```sh
   stellar contract deploy \
     --wasm target/wasm32-unknown-unknown/release/chv_token.wasm \
     --source <identity> \
     --network testnet
   ```
3. Invoke an initialization function:
   ```sh
   stellar contract invoke \
     --id <CONTRACT_ID> \
     --source <identity> \
     --network testnet \
     -- initialize \
     --admin <ADMIN_ADDRESS> \
     --treasury <TREASURY_ADDRESS>
   ```
4. To upgrade a deployed contract, call the `upgrade` function with the new WASM hash:
   ```sh
   stellar contract invoke \
     --id <CONTRACT_ID> \
     --source <identity> \
     --network testnet \
     -- upgrade \
     --admin <ADMIN_ADDRESS> \
     --new_wasm_hash <NEW_WASM_HASH>
   ```

## Contributing

1. Fork the repo and create a feature branch from `main`.
2. Make your changes and add tests for any new behaviour.
3. Ensure `cargo test` passes and `cargo clippy` reports no warnings.
4. Open a pull request against `main` with a clear description referencing any related issues.

Please follow the existing code style and keep commits focused and descriptive.
