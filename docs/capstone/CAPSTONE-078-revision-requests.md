# CAPSTONE-078: Request Proposal Revisions

## Scope
- Implements on-chain revision request primitive for CAPSTONE-078 (#1426).
- Contract: `contracts/capstone-revision-requests`.
- Supports tenant/project-scoped revision requests tied to exact proposal versions with deadline, response status, and resubmission tracking.

## On-chain Stores vs Off-chain Content
- On-chain: `tenant_id`, `project_id`, `request_id`, proposal version, requirement hash, deadline, requester/responder addresses, status, response hash, resubmission version, expected version, timestamps, role grants, bounded request count.
- Off-chain: proposal text, structured change-request details, attachments, discussion, reviewer narratives; only hashes/IDs/timestamps/states stored on-chain.

## Authorization
- `initialize(admin)` sets admin (one-time).
- `set_role` requires admin auth; stores scoped roles (`Requester`, `Responder`) per tenant/project/account.
- State-changing methods require caller `require_auth()` and stored scoped role checks; deny by default.
- `respond_revision`/`resubmit_proposal` restricted to stored responder; `close_request` restricted to stored requester.

## Idempotency
- `request_revision` requires `expected_version == 0` and is idempotent for identical `request_id` payloads; conflicting replay returns `RequestExists`.
- `respond_revision`, `resubmit_proposal`, and `close_request` require current `expected_version`; identical replay on terminal/already-applied state returns stored record, conflicting replay rejected (`AlreadyResponded`, `ResubmissionNotAllowed`, `VersionMismatch`).

## Errors
- Append-only `ContractError`: `AlreadyInitialized(1)`, `NotInitialized(2)`, `Unauthorized(3)`, `InvalidRole(4)`, `RequestExists(5)`, `RequestNotFound(6)`, `DeadlineExpired(7)`, `InvalidState(8)`, `VersionMismatch(9)`, `LimitReached(10)`, `InvalidDeadline(11)`, `AlreadyResponded(12)`, `ResubmissionNotAllowed(13)`.

## Events
- `CRR_ROLE`: tenant, project, account, role, active.
- `CRR_REQ`: tenant, project, request_id, proposal_version, deadline.
- `CRR_RESP`: tenant, project, request_id, response_hash.
- `CRR_RESUB`: tenant, project, request_id, resubmission_version.
- `CRR_CLOSE`: tenant, project, request_id.
- Events carry IDs/enums/hashes/versions/timestamps only.

## Privacy Boundaries
- No proposal content, reviewer comments, or personal/free-text data stored or emitted.
- Requirement/response commitments are hashes only; sensitive content remains off-chain.

## Dependencies
- Depends on tenant/project identity and proposal versioning established by upstream capstone/proposal workflows (off-chain and/or other contracts).
- Integrates with proposal submission/resubmission flows that supply proposal/resubmission version numbers.

## Rollout/Rollback
- Rollout: deploy contract, initialize admin, grant scoped requester/responder roles per tenant/project, wire backend to create/respond/close requests with deterministic `request_id` and `expected_version`.
- Rollback: disable off-chain entrypoints/integrations; on-chain records remain immutable/auditable. Redeploy only if storage schema migration is versioned separately.

## Corresponding Other-Repository Work
- Backend/services: request authoring UX/API, deadline enforcement notifications, response capture, canonical hashing of requirement/response payloads, retry-safe orchestration by `request_id`.
- Frontend: revision inbox, proposal-version diff context, response/resubmission actions, stale-state handling for `VersionMismatch`/deadline expiry.
- Indexing/analytics (if present): consume `CRR_*` events for timelines without exposing private content.
