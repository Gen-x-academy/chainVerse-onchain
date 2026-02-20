# Chainverse Smart Contracts

## Requirements

- Rust (latest stable)
- wasm32 target
- Soroban CLI

## Setup

rustup target add wasm32-unknown-unknown
cargo install --locked soroban-cli

## Build

cd contracts
cargo build --target wasm32-unknown-unknown --release

## Test

cargo test

## Deploy Locally

soroban network add local \
 --rpc-url http://localhost:8000/soroban/rpc \
 --network-passphrase "Standalone Network ; February 2017"

soroban keys generate alice

soroban contract deploy \
 --wasm target/wasm32-unknown-unknown/release/chainverse_core.wasm \
 --source alice \
 --network local
