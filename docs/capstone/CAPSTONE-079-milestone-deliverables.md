# CAPSTONE-079: Submit Milestone Deliverables

## Scope
- Implements on-chain milestone deliverable submission primitive for CAPSTONE-079 (#1427).
- Contract: `contracts/capstone-milestone-deliverables`.
- Records immutable, versioned submissions of files, links, datasets, commits, demonstrations, or text commitments against milestone requirements.

## On-chain Stores vs Off-chain Content
- On-chain: tenant/project/milestone/deliverable IDs, monotonic version, deliverable kind enum, content hash, requirement hash, submitter address, submission timestamp, scoped submitter roles, latest/count tracking.
- Off-chain: actual files, URLs, datasets, repository contents, demos, narrative text, and requirement descriptions; only hashes/IDs/enums stored on-chain.

## Authorization
- `initialize(admin)` one-time admin setup.
- `set_role` admin-only, scoped per tenant/project for `Submitter` role.
- `submit_deliverable` requires caller `require_auth()` and active scoped submitter role; deny by default.

## Idempotency
- Submission requires `version == latest + 1` and `expected_version == latest` (monotonic optimistic concurrency).
- Identical replay of the same deliverable payload/version returns existing record without duplicate side effects.
- Conflicting replay or stale expected version rejected with `AlreadySubmitted`/`VersionMismatch`.

## Errors
- Append-only `ContractError`: `AlreadyInitialized(1)`, `NotInitialized(2)`, `Unauthorized(3)`, `AlreadySubmitted(4)`, `NotFound(5)`, `VersionMismatch(6)`, `LimitReached(7)`, `InvalidVersion(8)`.

## Events
- `CMD_ROLE`: tenant, project, account, role, active.
- `CMD_SUB`: tenant, project, milestone_id, deliverable_id, version, kind.
- Events contain only IDs/enums/versions.

## Privacy Boundaries
- No file contents, links, commit messages, dataset payloads, demo media, or free-text descriptions on-chain.
- Only commitment hashes and minimal public metadata emitted.

## Dependencies
- Depends on upstream milestone definitions and requirement commitments managed off-chain and/or in related capstone contracts.
- Review/acceptance flows (CAPSTONE-080) consume submitted deliverable IDs, versions, and hashes.

## Rollout/Rollback
- Rollout: deploy/initialize, grant submitter roles, integrate submission clients to compute content/requirement hashes and use deterministic deliverable IDs with monotonic expected versions.
- Rollback: disable new submissions in off-chain flows; existing immutable on-chain submission records remain authoritative for audit/history.

## Corresponding Other-Repository Work
- Backend: artifact storage, canonical hashing, deliverable packaging, validation of requirement mappings, retry-safe submission client.
- Frontend: milestone submission UI, artifact upload/link capture, version/status display, stale submission conflict handling.
- Review systems: fetch on-chain submission commitments for acceptance decisions without accessing private payloads on-chain.
