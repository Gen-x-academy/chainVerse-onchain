# nft_certificate: status of issues #692–#695

Investigation notes on the four reported bugs against the current state of
[`contracts/certificates`](../contracts/certificates). No code was changed as
part of this doc — it's a status check to scope what actually still needs work.

## #692 — `next_token_id` reset on upgrade (instance storage)

**Status: already fixed.**

[`storage.rs`](../contracts/certificates/src/storage.rs) already keeps the
counter in `persistent()` storage, not `instance()`:

```rust
pub fn next_token_id(env: &Env) -> u64 {
    let key = DataKey::NextTokenId;
    let id: u64 = env.storage().persistent().get(&key).unwrap_or(0);
    env.storage().persistent().set(&key, &(id + 1));
    env.storage().persistent().extend_ttl(&key, MIN_TTL, MAX_TTL);
    id
}
```

This landed via PR #665/#666 (commit `aeacc20`, referenced there as "Fix
#628"). Persistent storage survives `update_current_contract_wasm`, so the
counter is not reset by an upgrade. No further action needed for this issue
specifically.

## #693 — soul-bound enforcement missing on transfer

**Status: already fixed.**

`Certificate` carries a `soul_bound: bool` field
([`types.rs`](../contracts/certificates/src/types.rs)), set to `true` in both
`mint` and `mint_certificate`. `transfer()` in
[`lib.rs`](../contracts/certificates/src/lib.rs) checks it before allowing a
transfer:

```rust
if cert.soul_bound {
    return Err(ContractError::SoulboundTransferNotAllowed);
}
```

This landed in the same PR (commit `aeacc20`, referenced there as "Fix
#629"). The error variant is named `SoulboundTransferNotAllowed` rather than
`SoulBoundToken` as suggested in the issue, but it's the same check with the
same effect. No further action needed for this issue specifically.

## #694 — `burn()` has no ownership check

**Status: not implemented.** There is no `burn` function anywhere in
`lib.rs`. A stub test file,
[`src/soulbound_test.rs`](../contracts/certificates/src/soulbound_test.rs),
already contains placeholder tests named `test_burn_soulbound_certificate`
and `test_burn_others_certificate_fails`, but it is **not wired into the
crate** (`lib.rs` never declares `mod soulbound_test;`) and its test bodies
don't call the contract at all — they're empty placeholders.

Still needed: add a `burn` entry point that requires the caller to be the
certificate's owner (via `require_auth()` on the stored recipient, not a
caller-supplied address) before removing the certificate from storage.

## #695 — `metadata_uri` has no format validation

**Status: not implemented.** `mint_certificate` accepts
`metadata_uri: Bytes` but never validates it and never stores it on the
`Certificate` struct — it's only forwarded into the mint event. There's no
`EmptyMetadataUri` / `MetadataUriTooLong` / `InvalidMetadataUriScheme` error
variant in [`errors.rs`](../contracts/certificates/src/errors.rs), and no
validation function anywhere in the crate.

Still needed: length/prefix validation (non-empty, ≤512 bytes,
`https://` or `ipfs://` prefix) before minting, with the three error variants
from the issue.

## Separate, pre-existing problem: the crate doesn't currently compile

While tracing these issues, found that `contracts/certificates` is currently
broken independent of #692–#695, from what looks like a bad merge between two
competing fix PRs (`e7daa74`/`ead8412` and `602b048`) that used incompatible
data models — one keyed certificates by `(Address, BytesN<32> course_id)`,
the other by `(Address, u64 course_id)` with a `wallet` field. The repo ended
up with `lib.rs` on the `BytesN<32>` model, but several test files still
targeting the `u64` model:

- `lib.rs::get_certificate` calls `storage::load_certificate(...)`, which
  doesn't exist — `storage.rs` only defines `get_certificate`. This alone
  fails the build.
- `tests/pause_tests.rs` and `tests/security_tests.rs` call
  `client.has_certificate(...)` and `client.revoke_certificate(...)`, neither
  of which exist on `CertificateContractClient` (the real methods are
  `get_certificate` and `revoke`), and pass `u64` course IDs where
  `BytesN<32>` is required.
- `tests/security_tests.rs` additionally defines
  `test_init_rejects_reinitialization` twice in the same file.
- `tests/access-control.rs` imports a `token::TokenContract` that has nothing
  to do with this crate — looks like a misplaced file.
- `tests/cross_contract.rs` and `tests/types.rs` test/define entirely
  unrelated contracts (`academy_rewards`, `messaging`, `trading`, a
  `Patient`/`Observation` FHIR-style model) — also look misplaced under
  `certificates/tests/`.

This is unrelated to #692–#695 and is a bigger cleanup on its own — flagging
it here rather than folding it into this fix, since it touches many files
outside the scope of these four issues.
