# Certificate Contract — NFT Model Spec (Issues #696–#699)

This document specifies the target design for `contracts/certificates` covering
issues **#696** (`verify_certificate`), **#697** (`get_certificates_by_student`),
**#698** (drop redundant ledger field), and **#699** (unit test suite).

## Current vs. target state

The contract in `contracts/certificates/src` today keys certificates by
`(wallet, course_id)` and has no `token_id`, no `owner` field distinct from
`wallet`, no `metadata_uri`, and no `burn`. `Certificate` currently has a single
`issued_at: u64` field — there is no `issued_ledger` to remove yet.

The four issues assume a token_id-addressed NFT model (matching the
`token_id` already referenced in the `CertificateMinted` event in
[contracts.md](contracts.md) and [events.md](events.md)). This doc specifies
that target model so implementation can proceed from a single source of truth.

## `Certificate` struct (target)

```rust
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Certificate {
    pub token_id: u64,
    pub owner: Address,
    pub course_id: u64,
    pub issued_at: u64,       // env.ledger().timestamp() — sole timestamp source
    pub metadata_uri: Bytes,
}
```

- No `issued_ledger` field. `issued_at` (Unix timestamp) is the one canonical
  timestamp; nothing derives from `env.ledger().sequence()`. (#698)
- `metadata_uri` is validated at mint time (non-empty, bounded length); an
  invalid value is rejected with `ContractError::InvalidMetadataUri`.

## Storage (target)

```rust
pub enum DataKey {
    Admin,
    Paused,
    Minter(Address),           // authorized-minter allowlist
    TokenCounter,              // next sequential token_id
    Certificate(u64),          // token_id -> Certificate
    StudentCerts(Address),     // owner -> Vec<u64>
}
```

- `TokenCounter` is a persistent (not instance) entry so the next `token_id`
  survives a contract upgrade — see the "persists across simulated upgrade"
  test in #699.
- `StudentCerts(owner)` is the per-student index consumed by
  `get_certificates_by_student`.

## Minting

- `mint(env, caller: Address, student: Address, course_id: u64, metadata_uri: Bytes) -> Result<u64, ContractError>`
- `caller.require_auth()`; `caller` must be present in the `Minter` allowlist
  (checked via a `require_minter` helper analogous to today's
  `storage::require_admin`). Unauthorized callers get `ContractError::Unauthorized`.
- Assigns `token_id` from `TokenCounter` (post-increment, so IDs are sequential
  and unique), stores the `Certificate`, appends `token_id` to
  `StudentCerts(student)`, and emits `chainverse:certificate:minted`
  with payload `(student, course_id, token_id)` per the existing event
  standard.

## `verify_certificate` (#696)

```rust
pub fn verify_certificate(env: Env, token_id: u64) -> VerificationResult
```

```rust
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationResult {
    pub valid: bool,
    pub owner: Address,
    pub course_id: u64,
    pub issued_at: u64,
    pub metadata_uri: Bytes,
}
```

- Looks up `DataKey::Certificate(token_id)` via `storage::load_certificate`
  (a read-only getter — no `.set`/`.extend_ttl` calls in this path).
- Existing token: `valid: true` plus the certificate's fields.
- Missing token: `valid: false` with the remaining fields defaulted
  (`Address` has no `Default`, so the miss path must construct the struct
  fields explicitly rather than relying on `..Default::default()` — the
  fallback in the issue's sample code doesn't compile as written since
  `Address` isn't `Default`).
- No storage writes on either branch — this is the acceptance criterion
  "pure read".

## `get_certificates_by_student` (#697)

```rust
pub fn get_certificates_by_student(env: Env, student: Address) -> Vec<u64>
```

- Returns `storage.persistent().get(&DataKey::StudentCerts(student)).unwrap_or(vec![&env])`.
- Empty `Vec` (not an error) when the student has no certificates.
- `add_to_student_index` runs inside `mint`, appending the new `token_id` and
  bumping the entry's TTL (`extend_ttl` with the contract's existing
  threshold/bump constants).
- `remove_from_student_index` runs inside `burn`: loads the index, removes the
  matching `token_id` (order doesn't need to be preserved — swap-remove is
  fine), and writes the shortened `Vec` back. This is what keeps the index
  correct after a burn per the issue's acceptance criteria.

## `burn`

Implied by #699's `test_burn_by_owner_succeeds` /
`test_burn_by_non_owner_fails`, needed to exercise the index-removal
criterion in #697:

```rust
pub fn burn(env: Env, caller: Address, token_id: u64) -> Result<(), ContractError>
```

- `caller.require_auth()`; caller must equal `Certificate.owner`, else
  `ContractError::Unauthorized`.
- Removes `DataKey::Certificate(token_id)` and updates
  `DataKey::StudentCerts(owner)`.

## `transfer` (soulbound)

Unchanged in spirit from the current contract: always returns
`ContractError::SoulboundTransferNotAllowed`, now taking `token_id` instead of
`(wallet, course_id)`.

## New error codes

| Code | Name                 | Description                              |
| ---- | -------------------- | ----------------------------------------- |
| 8    | InvalidMetadataUri   | `metadata_uri` empty or exceeds max length |

(1–7 unchanged from `contracts/certificates/src/errors.rs`.)

## Test plan (#699 — 11 tests, ≥85% coverage)

| Test | Behavior asserted |
| ---- | ------------------ |
| `test_mint_by_authorized_minter` | Address in the `Minter` allowlist can mint; returns a `token_id`; certificate readable afterward. |
| `test_mint_by_unauthorized_caller_fails` | Caller not in `Minter` allowlist → `Err(ContractError::Unauthorized)`, no state change. |
| `test_token_ids_are_sequential_and_unique` | Mint N certificates; assert `token_id`s are `0..N` (or `1..=N`) with no repeats. |
| `test_soul_bound_transfer_fails` | `transfer` always returns `Err(ContractError::SoulboundTransferNotAllowed)`. |
| `test_burn_by_non_owner_fails` | Non-owner calling `burn` → `Err(ContractError::Unauthorized)`; certificate still present. |
| `test_burn_by_owner_succeeds` | Owner burns; `get_certificate`-equivalent lookup returns `None`; token_id removed from `StudentCerts`. |
| `test_invalid_metadata_uri_rejected` | Empty/oversized `metadata_uri` at mint → `Err(ContractError::InvalidMetadataUri)`. |
| `test_verify_certificate_valid` | Minted token_id → `VerificationResult { valid: true, .. }` matching stored fields. |
| `test_verify_certificate_invalid_token_id` | Unminted token_id → `VerificationResult { valid: false, .. }`; assert no storage entry was created as a side effect. |
| `test_get_certificates_by_student` | Mint several certs to the same student (and one to a different student); assert the first student's list contains exactly their token_ids. |
| `test_token_id_counter_persists_across_simulated_upgrade` | Mint some certificates, simulate re-deploying the contract's WASM in place (Soroban's `register_contract_wasm`/upgrade test pattern) against the same storage, mint again, and assert the new `token_id` continues from where the counter left off rather than restarting at 0. |

## Acceptance-criteria mapping

- **#696**: valid/invalid branches ✓ via `verify_certificate` above; purity ✓
  since the function only calls storage getters.
- **#697**: populated list ✓, empty-vec-not-error ✓, index updated on burn ✓
  via `add_to_student_index` / `remove_from_student_index`.
- **#698**: no `issued_ledger` field (already true in current code — this
  spec keeps it that way rather than reintroducing then removing it); wasm
  size criterion is a build-output check (`wc -c target/.../certificates.wasm`
  before/after), not a code change by itself.
- **#699**: table above enumerates all 11 required tests.
