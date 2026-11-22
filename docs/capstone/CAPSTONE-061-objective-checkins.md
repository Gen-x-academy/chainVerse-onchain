# CAPSTONE-061: Track Objective Progress Check-ins

**Track:** Planning | **Component:** on-chain | **Issue:** #1409 | **Contract:** `contracts/capstone-objective-checkins`

## Scope

Record periodic, participant-authored progress check-ins without rewriting history. An objective
registers an owner, optional delegate, cadence, specification hash, and visibility. Each check-in
carries a status (`OnTrack`, `AtRisk`, `Blocked`, `Closed`), bounded confidence, evidence and report
hashes, and an optional reference to the check-in it supersedes. Check-ins are append-only up to 64
per objective, and objectives are limited to 64 per project.

## On-chain stores vs off-chain content

| On-chain | Off-chain (capstone service) |
| --- | --- |
| `Objective`: tenant, project, objective id, owner/delegate, cadence, spec hash, status, latest check-in id, count, state, version | Objective narrative, private status commentary, participant identity data |
| `Checkin`: id, filer, status, confidence bps, evidence/report hashes, supersedes, recorded time, version | Status report text, evidence documents, attachments |
| Check-in id list, role grants, events | Notifications and audit UI rendering |

Evidence and report fields are SHA-256 commitments. No prose, URI, memo, or private reference is
written to the ledger; confidence is bounded to 10,000 basis points.

## Authorization

- `initialize` is one-time and requires the administrator's `require_auth`.
- The administrator grants and revokes tenant/project-scoped `Planner` and `Participant` roles.
- Objective registration requires an active planner role; check-ins require `require_auth` from the
  owner or registered delegate plus an active participant role for the exact tenant/project.
- `WrongOwner` is returned for an unregistered filer, and `Unauthorized` for a missing or
  mismatched role. There is no admin override for filing a check-in.
- Deny is the default for all reads of missing objectives and check-ins.

## Idempotency and versioning

- Re-registering the same objective id and payload returns the stored objective; a stale version
  returns `VersionMismatch`.
- Replaying a check-in id with the same payload returns the stored record with no second event; a
  different payload or stale version returns `VersionMismatch`.
- A new check-in must respect the cadence unless it explicitly supersedes a prior check-in;
  invalid supersedes references, excessive confidence, and closed objectives are rejected with
  stable errors.
- Bounds: `MAX_OBJECTIVES = 64`, `MAX_CHECKINS = 64`, `MAX_CADENCE_SECONDS = 31_536_000`,
  `MAX_CONFIDENCE_BPS = 10_000`. TTLs extend on read and write.

## Errors

Append-only `ContractError` codes: `AlreadyInitialized=1`, `NotInitialized=2`, `Unauthorized=3`,
`InvalidRole=4`, `ObjectiveExists=5`, `ObjectiveNotFound=6`, `CheckinExists=7`, `CheckinNotFound=8`,
`WrongOwner=9`, `ObjectiveClosed=10`, `CadenceViolation=11`, `InvalidConfidence=12`,
`InvalidCadence=13`, `InvalidSupersedes=14`, `CheckinLimitReached=15`, `ObjectiveLimitReached=16`,
`VersionMismatch=17`, `LimitReached=18`. Codes are never renumbered.

## Events

| Symbol | Data |
| --- | --- |
| `OBC_ROLE` | `(tenant, project, account, role, active)` |
| `OBC_OBJ` | `(tenant, project, objective_id, owner)` |
| `OBC_CHK` | `(tenant, project, objective_id, checkin_id, status, confidence)` |

Status reports, evidence, and participant identity details are never emitted.

## Privacy boundaries

Progress commentary can reveal sensitive staffing or safety information, so only status enums,
bounded confidence, and hashes are published. Supersedes links form an append-only chain rather than
an overwrite, preserving the audit trail without storing the report text. Filer addresses are public
Stellar accounts and must not be derived from private identity data.

## Dependencies

- CAPSTONE-057 objectives supply the objective id, owner, delegate, and cadence configuration.
- CAPSTONE-059 schedule risk consumes status and confidence signals via the off-chain model.
- CAPSTONE-060 milestones may reference objective progress hashes for completion evidence.
- Role grants are mirrored from the shared tenant access-control service by the backend.

## Rollout and rollback

Enable behind the objective-checkins flag on testnet for one cohort, dual-writing check-ins to the
service database and the contract. Reconcile check-in counts and latest ids before promoting.
Rollback stops new check-ins; stored records are append-only and hash-only, so no private content
needs purging and a later deployment resumes from the last stored check-in.

## Other-repository work

- Backend: canonicalize and hash reports/evidence, mirror roles and owner/delegate configuration,
  manage cadence scheduling, and expose reconciliation and audit views.
- Frontend: check-in form, status/confidence display, supersedes chain view, and evidence links that
  remain access-controlled off-chain.
- Indexer/ops: monitor `OBC_CHK`, alert on `CadenceViolation` and `WrongOwner`, and renew TTLs for
  active objectives and check-ins.
