# Contributing to chainVerse

Thank you for your interest in contributing to chainVerse! This guide will help you get started and while you are here do well to join our Telegram at [**chainverse**](https://t.me/+nfr3_9fyvDozYzI0).

### Important Note Before Applying 📝

⚠️ **Avoid Generic Comments:** Comments such as 🚫
"Can I help with this?" 🚫
"I’d love to contribute!" 🚫
"Check out my profile!" or 🚫
"Can I work on this?"... these will not be considered.

Instead, provide a **clear explanation of your approach**, which includes:

- A brief introduction about yourself.
- A concise plan outlining how you will address the issue (3–6 lines max).
- Your estimated completion time (ETA).

## What is chainVerse?

ChainVerse Academy is a decentralized Web3 education platform built on the Stellar blockchain. It offers crypto-based payments, NFT certifications, and DAO governance, allowing students to learn about multiple blockchain ecosystems, earn rewards, and own their learning assets through secure, low-cost transactions.


## chainVerse Academy - Key Points

- Enable crypto-based course purchases and seamless Web3 wallet integration (e.g., Metamask, WalletConnect)
- Provide an instructor dashboard for uploading courses, setting crypto prices, and tracking student engagements
- Facilitate live learning sessions and 1-on-1 mentorship with smart contract-backed payments
- Conduct exams and assignments on-chain, offering crypto rewards for top-performing students
- Issue verifiable NFT certificates upon course completion, stored securely on the blockchain
- Allow users to transfer or resell courses through a smart contract-driven ownership model
- Implement a decentralized reputation system and DAO governance for community-led platform improvements

How to Contribute🤝

## Pull Request Template

To ensure consistency and improve the review process, we've implemented a PR template. When creating a pull request, please:

1. Follow the PR template that automatically loads when you create a new PR.

2. Fill out all relevant sections of the template.

3. Ensure your PR description clearly communicates the changes you've made.

4. Include screenshots or recordings when applicable.

5. Link to any related issues using keywords like "Closes #123" or "Fixes #123"

The template location is at [.github/PULL_REQUEST_TEMPLATE.md](.github/PULL_REQUEST_TEMPLATE.md) and provides a structured format to help maintainers understand and review your contribution more efficiently.

## Steps to apply

1. Apply for an Issue
   -Look for an open issue and comment expressing your interest in working on it.## Smart Contract Development

Smart contract changes live under `contracts/`. Use this workflow when you are
working on Soroban contracts, shared contract utilities, contract tests, or
deployment docs.

### Toolchain setup

1. Install Rust with `rustup` if it is not already available:

   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   rustup update
   ```

2. Install the WebAssembly target used by Soroban builds:

   ```bash
   rustup target add wasm32-unknown-unknown
   ```

   The repository also includes `rust-toolchain.toml`, which pins the stable
   Rust channel and requests `rustfmt`, `clippy`, and the WASM target.

3. Install the Stellar CLI. If your local toolchain still uses the legacy
   Soroban binary, the same network and contract commands may be available as
   `soroban`.

   ```bash
   cargo install --locked stellar-cli
   ```

### Build, test, and format

Run contract commands from the `contracts/` directory:

```bash
cd contracts
make build
make test
make fmt
make check
```

The Makefile maps these targets to the workspace commands contributors should
use before opening a PR:

- `make build` builds the full workspace for `wasm32-unknown-unknown`.
- `make test` runs `cargo test --workspace`.
- `make fmt` runs `cargo fmt --all`.
- `make check` runs `cargo clippy --workspace -- -D warnings`.

If you only change one contract, you can run focused checks with Cargo as well:

```bash
cd contracts
cargo test -p chainverse-core
cargo build -p chainverse-core --release --target wasm32-unknown-unknown
```

Replace `chainverse-core` with the contract crate you changed.

### Manual testnet deployment

Manual deployment is useful for validating a compiled WASM against a live
network before asking for review. Never commit private keys, seed phrases, or
funded account secrets.

```bash
cd contracts
make build

stellar network add testnet \
  --rpc-url https://soroban-testnet.stellar.org \
  --network-passphrase "Test SDF Network ; September 2015"

stellar keys generate deployer --network testnet

stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/chainverse_core.wasm \
  --source deployer \
  --network testnet
```

If your installed CLI exposes the older command name, replace `stellar` with
`soroban` and keep the same network passphrase and WASM path.

### Writing a new contract test

1. Put unit tests next to the contract module when the crate already follows
   that pattern, for example `contracts/<crate>/src/test.rs` or
   `contracts/<crate>/src/tests/`.
2. Put integration-style tests in `contracts/<crate>/tests/` when the crate
   already has that directory.
3. Use `soroban_sdk::Env::default()` and register the contract with the test
   environment before calling contract methods.
4. Cover both the success path and at least one failure path, especially for
   authorization, initialization, storage, and token-transfer behavior.
5. Run the focused crate test first, then the full workspace:

   ```bash
   cd contracts
   cargo test -p <crate-name>
   make test
   ```

For PRs that touch contract behavior, include the exact build/test commands you
ran and mention whether a testnet deployment was performed.

## Code of Conduct
   -Wait for the maintainer to assign the issue to you.
   -Remember to apply only if you can solve the issue.
   Again, In the comment, Add a quick introduction about yourself, The ETA, and how you plan to tackle the issue.

## Setup Instructions

1. Fork the repository.

2. Install the required Rust target for building WASM contracts:

   ```bash
   rustup target add wasm32-unknown-unknown
   ```

3. Clone your fork locally:

```bash
git clone https://github.com/your-username/chainVerse-onchain.git
cd onchain
```

4. In your forked repo, Create a new branch:

   ```bash
   git checkout -b feature/your-feature-name
   ```

5. Make your changes

6. Commit with clear messages:

```bash
git commit -m "Add: brief description of changes"
```

7. Push to your fork:

   ```bash
   git push origin feature-name
   ```

8. Submit a Pull Request that properly describes your changes

## Code of Conduct

By participating in this project, you agree to adhere to our [Code of Conduct](CODE_OF_CONDUCT.md).

## License

This project is licensed under the [MIT License](LICENSE).

## Contact

For inquiries, reach out to us on Telegram at [**chainverse**](https://t.me/+nfr3_9fyvDozYzI0).

---
