# Shared Crate

## Purpose

`shared` is a helper **library** crate, not a deployable Soroban contract — its
`Cargo.toml` builds only an `rlib` (no `cdylib`), and no `#[contract]`/`#[contractimpl]`
attributes appear anywhere in it. It exists to give other contracts in the workspace
reusable storage-access helpers, a common error enum, and event-publishing helpers.

> **Known issue:** as of the current code, no workspace member actually depends on
> `shared` (none of `certificates`, `chainverse-core`, or `escrow`'s `Cargo.toml` list it
> as a dependency). Two source references to a `shared` crate exist elsewhere in the
> repo — `contracts/certificates/tests/cross_contract.rs` (`shared::governance::...`) and
> files under `contracts/reward/` (`shared::errors::...`) — but neither module
> (`governance`, `errors`) exists in this crate (the real module is `error`, singular),
> and `contracts/reward/` has no `Cargo.toml` at all, so it isn't a buildable crate. Treat
> those as stale references, not active integrations.

## Entry points (public API)

This is a library, not a contract, so there are no auth-gated entry points — these are
plain functions any contract can call after adding `shared` as a dependency.

| Function | Signature | Purpose |
|---|---|---|
| `get_instance_storage` | `<K, V>(env: &Env, key: &K) -> Option<V>` | Read a value from instance storage |
| `set_instance_storage` | `<K, V>(env: &Env, key: &K, val: &V)` | Write/overwrite a key in instance storage |
| `remove_instance_storage` | `<K>(env: &Env, key: &K)` | Remove a key from instance storage (no-op if absent) |
| `get_persistent_storage` | `<K, V>(env: &Env, key: &K) -> Option<V>` | Read a value from persistent storage |
| `set_persistent_storage` | `<K, V>(env: &Env, key: &K, val: &V)` | Write/overwrite a key in persistent storage |
| `remove_persistent_storage` | `<K>(env: &Env, key: &K)` | Remove a key from persistent storage |
| `EventEmitter::course_purchased` | `(env: &Env, buyer: &Address, course_id: u64, amount: i128)` | Publish a course-purchase event |
| `EventEmitter::reward_claimed` | `(env: &Env, user: &Address, reward_id: u64, amount: i128)` | Publish a reward-claim event |
| `EventEmitter::certificate_minted` | `(env: &Env, user: &Address, course_id: u64, token_id: u64)` | Publish a certificate-minted event |
| `EventEmitter::emit_certificate_minted` | `(env: &Env, wallet: Address, course_id: u64, timestamp: u64)` | Publish a certificate-minted event (alternate topic/shape) |

Also exported: `ContractError` (the shared error enum, below). Note `types.rs` defines a
`Certificate { wallet, course_id, issued_at }` struct but it is **not** declared as a
module in `lib.rs`, so it's currently unreachable from outside the crate.

## Storage layout

There is no `DataKey` enum in this crate — the storage helpers are key-agnostic; callers
supply their own keys (e.g. a `Symbol`).

## Events

| Function | Topics | Data |
|---|---|---|
| `course_purchased` | `(symbol_short!("chainvrs"), symbol_short!("course"), symbol_short!("purchase"))` | `(buyer: Address, course_id: u64, amount: i128)` |
| `reward_claimed` | `(symbol_short!("chainvrs"), symbol_short!("reward"), symbol_short!("claimed"))` | `(user: Address, reward_id: u64, amount: i128)` |
| `certificate_minted` | `(symbol_short!("chainvrs"), symbol_short!("cert"), symbol_short!("minted"))` | `(user: Address, course_id: u64, token_id: u64)` |
| `emit_certificate_minted` | `(CERTIFICATE_MINTED, wallet: Address)` where `CERTIFICATE_MINTED = symbol_short!("CertMint")` | `(wallet: Address, course_id: u64, timestamp: u64)` |

## Error codes

| Variant | Code | Intended use |
|---|---|---|
| `Unauthorized` | 1 | Caller lacks required permission |
| `AlreadyPurchased` | 2 | Duplicate course purchase |
| `InvalidPayment` | 3 | Payment validation failed |
| `AlreadyRewarded` | 4 | Duplicate reward claim |
| `CertificateExists` | 5 | Duplicate certificate mint |
| `ContractPaused` | 6 | Action blocked while paused |
| `SoulboundTransferNotAllowed` | 7 | Attempted transfer of a non-transferable asset |

## Testing

```
cargo test -p shared
```

Unit tests only, in `src/storage.rs` (`#[cfg(test)] mod tests`), covering
instance/persistent get/set/remove/overwrite behavior. There are no integration test
targets and no test currently requires the crate's `testutils` feature.

## Example CLI invocations

Not applicable — `shared` has no `cdylib` target and cannot be deployed or invoked on
its own. To use it, add it as a path dependency in another contract's `Cargo.toml` and
call its functions directly from that contract's Rust code.
