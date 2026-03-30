# Developer Setup Guide

This comprehensive guide details how to setup the developer environment, build the smart contracts, execute tests, and run local deployments via Soroban.

## Prerequisites

- **Rust toolchain** (latest stable version): [Install Rust](https://www.rust-lang.org/tools/install)
- **Soroban CLI**
- A code editor containing rust-analyzer.

## Initial Setup Environment

1. Add the correct missing WebAssembly architectures the chain requires.
   ```bash
   rustup target add wasm32-unknown-unknown
   ```

2. Install the `soroban-cli` command-line utility for managing network activities.
   ```bash
   cargo install --locked soroban-cli
   ```

## Build Instructions

To compile the smart contracts safely locally, navigate to the `contracts` folder from the root repository:

```bash
cd contracts
cargo build --target wasm32-unknown-unknown --release
```

## Running Tests

To execute tests to see all module's assertions succeed, simply run `cargo test`:

```bash
cargo test
```

## Local Testing and Deployment (Soroban Network Local)

You might need to launch a devnet/local node. Setup your network mapping configuration for Soroban CLI:

```bash
soroban network add local \
  --rpc-url http://localhost:8000/soroban/rpc \
  --network-passphrase "Standalone Network ; February 2017"
```

1. Generate Alice's test keys to cover transaction fees and manage states:
   ```bash
   soroban keys generate alice
   // OR your custom identity:
   soroban keys add admin
   ```

2. Perform exactly the deployment strategy outlined via Soroban. You'll specify the source file compiled within the target directory:
   ```bash
   soroban contract deploy \
     --wasm target/wasm32-unknown-unknown/release/chainverse_core.wasm \
     --source alice \
     --network local
   ```

Ensure you keep updating your `rust` toolchain `rustup update` for any breaking changes that Soroban may introduce in protocol updates.
