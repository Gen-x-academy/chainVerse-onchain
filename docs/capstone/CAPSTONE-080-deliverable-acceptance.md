# CAPSTONE-080: Review and Accept Milestone Deliverables

## Scope
- Implements on-chain deliverable review/acceptance primitive for CAPSTONE-080 (#1428).
- Contract: `contracts/capstone-deliverable-acceptance`.
- Allows designated reviewers to record reasoned decisions for current submissions: request changes, accept, or reject, with rubric criteria commitment.

## On-chain Stores vs Off-chain Content
- On-chain: tenant/project/milestone/deliverable IDs, submission version, rubric hash, reason hash, decision enum, status, reviewer address, decision timestamp, expected version marker, scoped reviewer roles, bounded decision counts.
- Off-chain: rubric text/criteria details, review comments, evidence attachments, and private deliberation; only hashes/IDs/enums stored on-chain.

## Authorization
- `initialize(admin)` one-time.
- `set_role` admin-only for scoped `Reviewer` role per tenant/project.
- `decide` requires reviewer `require_auth()` and active scoped reviewer role; deny by default.

## Idempotency
- Initial decision requires `expected_version == 0`.
- Identical replay of the same decision payload for the same deliverable returns existing record (no duplicate side effects).
- Conflicting re-decision or different payload for an already decided deliverable rejected with `AlreadyDecided`; stale expected version rejected with `VersionMismatch`.

## Errors
- Append-only `ContractError`: `AlreadyInitialized(1)`, `NotInitialized(2)`, `Unauthorized(3)`, `DecisionExists(4)`, `NotFound(5)`, `VersionMismatch(6)`, `LimitReached(7)`, `InvalidState(8)`, `AlreadyDecided(9)`.

## Events
- `CDA_ROLE`: tenant, project, account, role, active.
- `CDA_DEC`: tenant, project, milestone_id, deliverable_id, submission_version, decision.
- Events include only IDs/enums/versions.

## Privacy Boundaries
- No review narratives, rubric prose, scores with sensitive context, or personal data on-chain.
- Reason and rubric represented as hashes only.

## Dependencies
- Depends on deliverable submissions (CAPSTONE-079) to identify current submission versions.
- May inform funding/milestone release logic in separate settlement/escrow components.

## Rollout/Rollback
- Rollout: deploy/initialize, assign reviewer roles, integrate review workflows to submit hash-committed rubric/reason decisions with deterministic identifiers and expected versions.
- Rollback: disable new on-chain decisions in off-chain workflows; existing decisions remain immutable audit records.

## Corresponding Other-Repository Work
- Backend: review assignment/workflow APIs, rubric/reason storage and hashing, notification fanout, conflict-safe retry handling.
- Frontend: reviewer decision UI, rubric presentation, decision history/status views, user-facing error mapping for stale/conflicting decisions.
- Reporting/indexing: consume `CDA_DEC` events for milestone progress dashboards without exposing private review content.
