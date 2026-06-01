#!/bin/bash
set -e

cd contracts
cargo build --workspace --release --target wasm32-unknown-unknown

echo "Build complete. WASM files:"
find target/wasm32-unknown-unknown/release -name "*.wasm" | sort
