# Capstone Safety and Integrity

On-chain primitives for **CAPSTONE-098** (research ethics screening),
**CAPSTONE-099** (integrity concern reports), **CAPSTONE-100** (audited case
triage and priority) and **CAPSTONE-101** (protective measures).

## Privacy boundary

Protocols, report narratives, triage notes and measure reasons stay off-chain.
The contract stores only opaque ids, addresses, bounded 32-byte commitments,
u32 codes, counters and timestamps. Events contain ids and codes, never private
payloads.

## Authorization

`initialize(admin)` is one-time. The global admin only grants/revokes
tenant-scoped roles. Every capability write requires `caller.require_auth()` and
an explicitly granted role for that `tenant_id`; there is no implicit admin
fallback. Roles: 1 submitter, 2 reviewer, 3 responder, 4 operator, 5 auditor.

## Idempotency and lifecycle

Mutations bundle bounded fields in `CapstoneInput` (including `request_id`,
`input_hash` and `expected_version`) to stay within Soroban's 10-parameter ABI.
Replays return the stored version; conflicting inputs fail with
`RequestConflict`. Stale updates fail with `StaleVersion`. Error codes are
append-only.

## Capabilities

- **CAPSTONE-098:** screening submissions require protocol, consent and risk
  commitments. Approval grants a time-bounded clearance; material changes move
  the record back to `Changes`, so `ethics_clearance_active` gates activation.
- **CAPSTONE-099:** reports store evidence/safe-contact commitments and a
  category code, then move Filed → Triaged → Escalated/Closed. No narrative is
  stored on-chain.
- **CAPSTONE-100:** `open_case` links a triaged report, computes priority from
  severity, age and conflict/funding flags, and sets an SLA deadline. Access is
  audited with a hash and counter only.
- **CAPSTONE-101:** protective measures are time-bound, scoped to a capability
  (publishing, messaging, files, funding, review, participation), linked to a
  triaged case, and can be lifted with an idempotent request.

## Off-chain boundaries / dependencies

The ethics protocol store, report inbox, case management tool, notification
service and enforcement points are off-chain dependencies. Their data enters
this contract only as salted 32-byte commitments; the contract never calls
them.

## Rollout and rollback

Deploy behind the standard upgrade authority. A fresh deployment starts with
an empty role table. To roll back, pause callers at the integration layer and
redeploy the previous WASM; no private content lives on-chain.

## Corresponding work

CAPSTONE-098..101 are shared work items implemented across the chainVerse
repositories; this crate provides only their on-chain commitment and state
primitive.

No contract tests were added in this change, per bounty scope.
