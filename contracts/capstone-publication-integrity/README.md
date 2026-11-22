# Capstone Publication Integrity

On-chain primitives for **CAPSTONE-094** (credentials), **CAPSTONE-095**
(output verification), **CAPSTONE-096** (reproducible exports) and
**CAPSTONE-097** (originality/citation checks).

## Privacy boundary

Only opaque ids (`BytesN<32>`), addresses, u32 state/reason codes, counters and
timestamps are stored. Credential text, output payloads, export archives,
licenses, reproducibility instructions, similarity reports and citation text
stay off-chain; the contract stores only bounded commitments to them. Events
carry ids and codes, never free text or payload preimages.

## Authorization

`initialize(admin)` is one-time. The global admin can only grant/revoke
tenant-scoped roles; every capability entrypoint requires `caller.require_auth()`
and an explicitly granted role for that `tenant_id`. There is no implicit
admin fallback. Roles: 1 issuer, 2 verifier, 3 curator, 4 reviewer.

## Idempotency and lifecycle

Mutations bundle their bounded fields in a `CapstoneInput` struct (including
`request_id`, `input_hash` and `expected_version`), keeping each entrypoint
within Soroban's 10-parameter ABI limit. A replay with the same request pair
returns the stored version without changing state; a conflicting pair fails
with `RequestConflict`. Updates check `expected_version`, so stale transitions
fail with `StaleVersion`. Unknown ids fail with `NotFound`; invalid transitions
fail with `InvalidState`. Error codes are append-only.

## Capabilities

- **CAPSTONE-094:** `mint_credential` requires commitments to completed rules
  and contributor approvals; `revoke_credential` records a reason hash.
- **CAPSTONE-095:** `set_output_state` enforces the verification state graph;
  `get_output_state` returns `0` for Unknown without exposing participant data.
- **CAPSTONE-096:** `prepare_export` binds package/checksum/license hashes and
  a bounded expiry; `grant_export_access` / `revoke_export_access` manage an
  expiring access grant, and `export_access_active` checks the deadline.
- **CAPSTONE-097:** `request_similarity_check` stores a restricted report ref,
  `record_similarity_score` applies a provider-independent threshold in basis
  points, and `finalize_similarity_check` records a human review decision.

## Off-chain boundaries / dependencies

The similarity provider, report store, package store, identity/approval service
and notification system are off-chain dependencies. Their outputs enter this
contract only as salted 32-byte commitments; the contract never calls them.

## Rollout and rollback

Deploy behind the repo's standard upgrade authority. A new deployment starts
with an empty role table and no records. To roll back, pause callers at the
integration layer and redeploy the previous WASM; no private data is stored on
this contract, so no on-chain erasure step is required.

## Corresponding work

The shared work items CAPSTONE-094..097 are implemented in the other
chainVerse repositories; this crate provides only the on-chain commitment and
state primitive for each.

No contract tests were added in this change, per bounty scope.
