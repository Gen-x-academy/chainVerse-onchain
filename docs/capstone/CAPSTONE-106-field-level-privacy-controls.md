# CAPSTONE-106: Add Project Field-Level Privacy Controls

**Track:** Privacy | **Component:** on-chain | **Issue:** #1454 | **Contract:** `contracts/capstone-field-privacy`

## Scope

Store only visibility decisions for capstone project fields. The nine controlled fields are
`Profile`, `Proposal`, `Membership`, `Notes`, `Progress`, `Outputs`, `Location`, `Funding`, and
`Achievements`. A rule grants audiences (`Public`, `Participants`, `Supervisors`, `Reviewers`,
`Sponsors`, `Administrators`) access to a single field for one subject in one tenant/project pair.
No field value, document, message, location, financial reference, or private note is written to the
ledger.

## On-chain stores vs off-chain content

| On-chain | Off-chain (capstone service) |
| --- | --- |
| `FieldRule`: tenant, project, subject address, field enum, audience booleans, expiry, state, version | Profiles, proposals, notes, progress reports, outputs, precise locations, funding records, achievements |
| `FieldPolicy`: tenant, project, field enum, allowed audience booleans, state, version | Policy rationale, legal basis, reviewer notes, policy UI copy |
| Tenant/project-scoped audience role grants | Identity data, account-to-person mapping, session and access tokens |
| Idempotency receipts, counters, bounded events | Audit UI rendering, notifications, off-chain query cache |

The service holds the content and serves it only after calling `is_visible`. A missing rule, a revoked
rule, an expired rule, or a missing audience grant denies access.

## Authorization

- `initialize` is one-time and requires the administrator's `require_auth`.
- The administrator grants or revokes audience roles for an exact `(tenant_id, project_id)` pair.
  There is no implicit admin read override; an administrator role must be granted explicitly.
- `set_visibility`, `revoke_visibility`, and their aliases require `require_auth` from the subject
  that owns the field. A subject cannot set another subject's rule.
- `set_policy`, `freeze_policy`, and `unfreeze_policy` require the admin address.
- `is_visible` is a permissionless decision function: the viewer address is checked against stored
  audience grants and the subject's rule. It never calls `require_auth` because the decision itself
  discloses no private field value.

## Lifecycle, idempotency, and versioning

- A rule is created with `expected_version = 0`, progresses through active updates, and can be
  revoked. Reactivation is an explicit update with the current version.
- A policy is created with version 1, can be frozen and unfrozen by the admin, and cannot be changed
  while frozen.
- Every mutating call carries a `request_id`. An identical retry returns the stored record without a
  second event; the same id with a different payload returns `RequestConflict`.
- Stale versions return `StaleVersion`. Reusing a request id after a successful mutation returns the
  recorded version, so concurrent requests and retries cannot duplicate side effects.
- `valid_until = 0` means no expiry; a past or current timestamp is rejected with `InvalidExpiry`.
  After expiry, the rule denies all viewers.
- Bounds: `MAX_RULES_PER_PROJECT = 256`, `MAX_REQUESTS_PER_ACTOR = 256`, and six audience grants per
  account. Persistent records extend TTL on read and write.

## Errors

Append-only `ContractError` codes: `NotInitialized=1`, `AlreadyInitialized=2`, `Unauthorized=3`,
`PolicyLimit=4`, `RuleNotFound=5`, `PolicyNotFound=6`, `StaleVersion=7`, `RequestExists=8`,
`RequestConflict=9`, `LimitReached=10`, `Expired=11`, `PolicyFrozen=12`, `InvalidExpiry=13`,
`InvalidTransition=14`, `RuleRevoked=15`, `InvalidInput=16`. Codes are never renumbered.

`Expired`, `InvalidInput`, and `RequestExists` are reserved for adjacent lifecycle and receipt
checks; they are not removed or renumbered.

## Events

| Symbol | Data |
| --- | --- |
| `CAPFLDROL` | `(tenant, project, account, audience, active)` |
| `CAPFLDPOL` | `(tenant, project, field, allowed, version)` or `(tenant, project, field, state, version)` |
| `CAPFLDRUL` | `(tenant, project, subject, field, allowed, valid_until, state, version)` |
| `CAPFLDREV` | `(tenant, project, subject, field, state, version)` |

Events carry only ids, enums, audience booleans, timestamps, states, and versions. No field value,
hash of private content, URI, memo, or audit narrative is emitted.

## Privacy boundaries

- The blockchain stores decisions, never content. The subject address is a public Stellar account
  and must not be derived from private identity data.
- Public visibility is opt-in and always subject to the admin policy. A policy can never grant
  access by itself; a subject rule must also allow the audience.
- Field values remain encrypted in the off-chain store. The service must call `is_visible` before
  returning a field and must not log field values into contract events.
- Exact location, unpublished IP, evidence keys, datasets, and payout references are outside this
  primitive; they are covered by CAPSTONE-109 and remain off-chain.

## Dependencies

- CAPSTONE-014 profile visibility supplies the existing profile audience model; this contract
  generalizes it to all project fields with tenant/project scoping.
- CAPSTONE-105 consent ledger governs purpose-specific processing; `is_visible` is not consent and
  must be combined with an active consent decision where required.
- CAPSTONE-107 export and CAPSTONE-108 erasure consume these decisions off-chain to filter results
  and coordinate deletion without writing content to the ledger.
- Tenant role grants are mirrored from the shared access-control service by the backend.

## Rollout and rollback

Enable behind the field-privacy flag on testnet for one cohort. Backfill off-chain defaults as
deny-all, then migrate subject-selected settings through idempotent calls with new request ids.
Reconcile rule counts and event versions before promoting. Rollback stops new rule writes and leaves
existing rules in place; the off-chain service treats the flag as deny-all during rollback, so no
private payload needs purging from the ledger. A later deployment resumes from stored versions.

## Other-repository work

- Backend: store field content, enforce `is_visible` at read time, mirror tenant/project audience
  roles, manage policy metadata off-chain, and expose reconciliation views.
- Frontend: per-field privacy controls with deny-by-default copy and audience previews.
- Indexer/ops: monitor `CAPFLD*` topics, alert on `PolicyLimit`, `PolicyFrozen`, `StaleVersion`, and
  `RequestConflict`, and renew TTLs for active projects.
