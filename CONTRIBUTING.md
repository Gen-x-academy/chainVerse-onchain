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
5. Link to any related issues using keywords like "Closes #123" or "Fixes #123".

The template location is at `.github/PULL_REQUEST_TEMPLATE.md` and provides a structured format to help maintainers understand and review your contribution more efficiently.

## Steps to apply

1. Apply for an issue.
   - Look for an open issue and comment expressing your interest in working on it.
2. Wait for the maintainer to assign the issue to you.
3. Remember to apply only if you can solve the issue.
4. In the comment, add a quick introduction about yourself, the ETA, and how you plan to tackle the issue.

## Setup Instructions

1. Fork the repository.

2. Install the required Rust target for building WASM contracts:

   ```bash
   rustup target add wasm32-unknown-unknown
   ```

## Contributing to Soroban Contracts

This section is for anyone adding or modifying a Rust/Soroban smart contract under `contracts/`.

### Toolchain setup

The pinned toolchain is defined in [`rust-toolchain.toml`](rust-toolchain.toml) — `rustup` picks it up automatically in this repo, so you don't need to install anything extra beyond the target and CLI below.

```bash
rustup target add wasm32-unknown-unknown
rustup component add rustfmt clippy

# Stellar CLI (used to build/deploy/invoke contracts)
cargo install --locked stellar-cli --version 21.0.0 --features opt
```

### Build

```bash
stellar contract build
```

This compiles every contract crate to `target/wasm32-unknown-unknown/release/*.wasm`.

### Test

```bash
cargo test --workspace
```

Run a single contract's tests while iterating:

```bash
cargo test -p chv_token
```

### Lint

```bash
cargo clippy --workspace -- -D warnings
```

CI treats clippy warnings as errors — run this locally before opening a PR.

### Writing a new contract function — checklist

Every state-changing function added to a contract must satisfy all of the following:

- [ ] Call `.require_auth()` on the relevant `Address` for any state-changing function
- [ ] Return a typed `Result<T, ContractError>` — no `unwrap()`, `expect()`, or `panic!()`
- [ ] Emit an event via `env.events().publish(...)` describing what changed
- [ ] Bump TTL (`extend_ttl`) on any persistent storage entry the function writes to
- [ ] Add a unit test covering the success path and each error case

Example, based on `contracts/chv_token/src/lib.rs`:

```rust
pub fn mint(env: Env, to: Address, amount: i128) -> Result<(), TokenError> {
    if amount <= 0 {
        return Err(TokenError::InvalidAmount);              // typed error, no panic
    }
    let admin: Address = env.storage().instance().get(&DataKey::Admin)
        .ok_or(TokenError::NotInitialized)?;
    admin.require_auth();                                    // auth check

    let balance: i128 = env.storage().persistent()
        .get(&DataKey::Balance(to.clone())).unwrap_or(0);
    env.storage().persistent().set(&DataKey::Balance(to.clone()), &(balance + amount));
    env.storage().persistent()
        .extend_ttl(&DataKey::Balance(to.clone()), BALANCE_MIN_TTL, BALANCE_MAX_TTL); // TTL bump

    env.events().publish((symbol_short!("MINT"),), (to, amount));                     // event
    Ok(())
}
```

### Contract upgrade procedure

Contracts are upgraded by deploying new WASM and invoking the contract's own `upgrade` function with the new WASM hash — the contract address stays the same:

```bash
stellar contract upgrade \
  --id <CONTRACT_ID> \
  --source <identity> \
  --network testnet \
  -- upgrade \
  --admin <ADMIN_ADDRESS> \
  --new_wasm_hash <NEW_WASM_HASH>
```

The `upgrade` function must `require_auth()` on the admin before installing the new WASM.

### PR checklist for contract changes

- [ ] `stellar contract build` succeeds with no errors
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace -- -D warnings` reports no warnings
- [ ] New/changed functions follow the checklist above (auth, typed errors, events, TTL, tests)
- [ ] PR description explains the storage layout or event schema, if changed