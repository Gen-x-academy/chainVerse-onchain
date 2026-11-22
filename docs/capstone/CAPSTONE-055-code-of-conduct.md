# CAPSTONE-055: Enforce Capstone Code of Conduct

**Track:** Governance | **Component:** on-chain | **Issue:** #1403 | **Contract:** `contracts/capstone-conduct`

## Scope

Enforce current conduct-policy acceptance without publishing conduct content. Policies are versioned
by hash, scoped by applicability (`Participant`, `Coordinator`, `Both`), and move from `Active` to
`Retired`. Participants acknowledge the policy that applies to them, re-acknowledge when a new
policy version is published, and breach records carry only a restricted reference hash and severity
enum (`Low`, `Medium`, `High`) with an `Open`/`Resolved` lifecycle.

## On-chain stores vs off-chain content

| On-chain | Off-chain (capstone service) |
| --- | --- |
| `Policy`: policy id, version hash, applicability, severity scale, effective time, state | Policy text, translations, examples, contact channels |
| `Acceptance`: account, policy id, acknowledged version hash, timestamps | Signed acknowledgement forms and identity documents |
| `BreachRef`: subject, policy id, reference hash, severity, state | Incident reports, interviews, investigation notes, sanctions rationale |
| Role grants and events | Notifications and case-management UI |

Policy, evidence, and incident fields are SHA-256 commitments over canonical off-chain records.
No free text, URI, memo, or private reference is written to the ledger.

## Authorization

- `initialize` is one-time and requires the administrator's `require_auth`.
- The administrator grants and revokes tenant/project-scoped `Participant` and `Coordinator` roles.
- Policy publication and retirement require the admin role; acceptance requires the acknowledging
  account's own `require_auth` and an applicable active role.
- Breach recording and resolution require a coordinator role for the exact tenant and project.
  A caller without the role, with a revoked role, or acting on another tenant gets `Unauthorized`.
- Deny is the default: `requires_acknowledgement` and `is_compliant` return false for unknown or
  retired policy contexts rather than granting compliance.

## Idempotency and versioning

- Replaying an acceptance for the same account, policy, and version hash is a no-op with no second
  event; a mismatched hash returns `VersionMismatch`.
- Publishing a policy id twice with the same content is idempotent; reusing an id with different
  content returns `PolicyNotFound`/conflict-style validation errors instead of overwriting history.
- Breach references are unique per `(subject, policy, reference hash)`; a retry returns the stored
  record and `BreachExists` is returned for a duplicate with different data.
- Bounds: `MAX_POLICIES = 32`, `MAX_BREACHES_PER_SUBJECT = 16`. Effective times must be present;
  future policies return `EffectiveAtNotReached`.

## Errors

Append-only `ContractError` codes: `NotInitialized=1`, `AlreadyInitialized=2`, `Unauthorized=3`,
`InvalidRole=4`, `PolicyNotFound=5`, `PolicyRetired=6`, `NotApplicable=7`, `ZeroHash=8`,
`AcceptanceNotFound=9`, `BreachExists=10`, `BreachNotFound=11`, `BreachLimitReached=12`,
`PolicyLimitReached=13`, `EffectiveAtNotReached=14`. Codes are never renumbered.

## Events

| Symbol | Data |
| --- | --- |
| `CONROLE` | `(tenant, project, account, role, active)` |
| `POLPUB` / `POLRET` | `(tenant, project, policy_id, version_hash)` |
| `POLACC` | `(tenant, project, policy_id, account, version_hash)` |
| `BRCHREC` / `BRCHRES` | `(tenant, project, policy_id, subject, reference_hash, severity)` |

No policy prose, incident narrative, or identity data appears in events.

## Privacy boundaries

Conduct acceptance is an attestation, not a disclosure. Only hashes and enums are stored on-chain;
the investigation record, complainant identity, and sanctions reasoning remain in the access-controlled
off-chain case system. Restricted breach references are opaque hashes, and the contract exposes no
mapping from reference hash to subject identity.

## Dependencies

- CAPSTONE-050 charter acceptance supplies the participant set that must acknowledge the code.
- CAPSTONE-053 pause/resume may suspend a participant while a breach is open.
- CAPSTONE-054 project closures consume a compliance summary; CAPSTONE-056 governance export
  includes acceptance state without breach details.
- Role grants are mirrored from the shared tenant access-control service by the backend.

## Rollout and rollback

Publish policies on testnet behind the conduct-enforcement flag, dual-writing acknowledgements to the
service database and the contract for one cohort. Promote only after acknowledgement counts match.
Rollback disables new acceptance and breach calls but leaves existing hash-only records readable;
no private content needs purging, and a later deployment can resume from the last stored version.

## Other-repository work

- Backend: canonicalize and hash policy and incident documents, mirror roles, manage effective
  dates, and expose compliance checks plus restricted case management.
- Frontend: policy acknowledgement flow, re-acknowledgement prompts, and compliance status display.
- Indexer/ops: monitor `POLPUB`/`BRCHREC`, alert on `NotApplicable`, and renew TTLs for active
  policies and breaches.
