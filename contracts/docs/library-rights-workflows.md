# Library-rights workflows

This guide describes the new e-library primitives delivered for issues #952–#954. The workflows are intentionally split between the `library_licensing` contract, which owns licenses and borrower grants, and the `library-rights` contract, which owns catalog integrity and patron attestations.

## Rendition migration

`LibraryLicensing::propose_rendition_migration(from_work_id, to_work_id, policy)` creates a one-way, admin-governed relationship between two rendition commitments. `RenditionMigrationPolicy::Forced` makes active grants usable for the successor immediately. `OptIn` preserves borrower choice until the grant holder calls `accept_rendition_migration(grant_id, to_work_id)`. `is_grant_active_for_work` checks the original validity window, license status, grant window, and migration policy; it never extends an expired or revoked grant. The original license and grant remain stored, so both rendition commitments are auditable and seat/grant accounting is conserved rather than duplicated.

The new `RenditionMigration` record is stored under the source commitment and opt-ins are stored per grant/successor pair. `REND_MIG` and `MIG_ACCPT` events provide an append-only operational trail. Deployment is additive for this pre-release contract: no existing key is removed or reshaped. Consumers should use the generated client bindings after rebuilding the contract.

## Integrity quarantine

`LibraryRightsContract::quarantine_work` is restricted to the bootstrapped `Emergency` role. It records only a `reason_hash`, timestamp, and emergency actor in `QuarantineRecord`, changes the separate `ContentStatus` to `Quarantined`, and makes `is_work_accessible` return false immediately. It does not overwrite the content hash, delete the work, or convert the state into a legal takedown. A legal takedown cannot be quarantined again, and ordinary deactivation remains a separate policy-manager transition.

Restoration requires the `PolicyManager` role and a `review_hash` through `restore_quarantined_work`. The original quarantine record is retained and augmented with restoration evidence and timestamp. `QUARANTIN`, `QUAR_REST`, and `WRK_STATE` events make the state transition auditable. Hashes are used instead of incident text or personal data, so forensic evidence can be retained off-chain without publishing it on-chain.

## Pseudonymous membership attestations

`attest_membership` accepts a wallet, an opaque `claim_commitment`, an `institution_domain_hash`, a `network_id`, and an exclusive `expires_at`. It stores no name, student number, email, plaintext claim, or institutional record. The record ID is derived from the wallet, commitments, network scope, and a monotonic nonce. `is_membership_active` requires the current wallet pointer and the same claim, institution, and network commitments, so a proof cannot be replayed under another institution or network context.

Issuing a new attestation automatically revokes the wallet’s previous current attestation while retaining the old record for audit. `revoke_membership` supports explicit revocation. The issued and revoked records are keyed separately from the current wallet pointer, and `MEM_ISSUE`/`MEM_REVOK` events expose only opaque record IDs and timestamps.

## ABI and migration notes

The `library-rights` ABI version is bumped to `0.5.0`, and `SCHEMA_VERSION` is bumped to `2` because the versioned key set now includes content status, quarantine evidence, membership records, and current-wallet pointers. Existing `Work` records retain their shape and default to `ContentStatus::Active` when no status key exists, which makes the additive state transition backward-compatible. The new `library_licensing` keys are additive and do not alter existing license or grant records.

All new persistent records use the established catalog TTL tier. Deployments should publish the updated WASM and generated interface specification together, then run the positive, negative, authorization, and boundary tests before enabling the workflow in application code.
