# CAPSTONE-038: Submit Proposals Atomically

**Track:** Proposals | **Component:** on-chain | **Status:** specified (not yet implemented)
**Issue:** #1386 | **Audit baseline:** `main` @ `5b62b369` (2026-11-18), revalidated against `main` @ `4471755`

Goal: submit capstone/research proposals atomically, freezing the accepted form
and call versions, validating every requirement, preventing duplicate active
submissions, and recording replay-safe intent.

## Revalidation against main

`main` has no proposal submission primitive. Related building blocks that this
work depends on:

| Contract | Used for |
| --- | --- |
| `capstone-identifiers` | `public_id` of the project/proposal and per-version references |
| `capstone-call-terms` | `latest_version(call_id)` / `get_version` — the call terms version that is frozen on submit |
| `capstone-delivery-modes` | `is_allowed(call_id, mode)` — rejects a mode the call does not offer |
| `capstone-opportunity-details` | Opportunity open/closed state and deadline |

## On-chain scope

A new contract, `contracts/capstone-proposals`, exposes `submit_proposal`
as a single invocation that either writes the full record or nothing.

```text
submit_proposal(
    submitter: Address,          // require_auth()
    tenant_id: BytesN<32>,
    call_id: BytesN<32>,
    proposal_id: BytesN<32>,     // public_id from capstone-identifiers
    form_version: u32,           // version the client rendered
    terms_version: u32,          // call terms version the submitter accepted
    content_hash: BytesN<32>,    // SHA-256 over canonical off-chain proposal
    team_ref: Option<BytesN<32>>,// CAPSTONE-041 team id, if any
    mode: u32,                   // delivery-mode bit
    intent_id: BytesN<32>,       // client idempotency key
) -> Result<u64 /* submission seq */, ProposalError>
```

The stored `Submission` record is bounded and holds no private content:

| Field | Type | Notes |
| --- | --- | --- |
| `proposal_id` | `BytesN<32>` | Opaque public id |
| `tenant_id` | `BytesN<32>` | Tenant boundary key |
| `call_id` | `BytesN<32>` | |
| `submitter` | `Address` | |
| `form_version` | `u32` | Frozen at submit |
| `terms_version` | `u32` | Frozen at submit; must equal `latest_version` |
| `content_hash` | `BytesN<32>` | Commitment only |
| `team_ref` | `Option<BytesN<32>>` | |
| `status` | `ProposalStatus` | `Submitted` on success |
| `revision` | `u32` | `1` for a first submission (see CAPSTONE-039) |
| `submitted_at` | `u64` | Ledger timestamp |

### Validation (all checks run before any write)

1. `submitter.require_auth()`; submitter is an eligible participant in `tenant_id`
   (deny by default — no role, no submit).
2. Call exists in this tenant, is open, and `now <= deadline` (equality allowed).
3. `terms_version == capstone-call-terms.latest_version(call_id)`; a stale
   version fails with `TermsVersionStale` so the client re-prompts acceptance.
4. `form_version` is the call's current published form version, otherwise `FormVersionStale`.
5. `content_hash` is non-zero.
6. `mode` is allowed by `capstone-delivery-modes`.
7. If `team_ref` is set, the team is `Formed` (CAPSTONE-041) and the submitter is its initiator or an accepted member.
8. No active submission exists for `(tenant_id, call_id, submitter)` or for
   `(tenant_id, call_id, team_ref)` — `DuplicateActiveSubmission`.

### Replay-safe intent

- `Intent(tenant_id, submitter, intent_id)` maps to the submission sequence.
- If `intent_id` is replayed with the **same** payload hash, the call returns
  the original sequence and emits no new event or side effect.
- If `intent_id` is replayed with a **different** payload, it fails with `IntentConflict`.
- Intent entries live in temporary storage with a TTL of at least the call's
  submission window, so storage stays bounded.

## Lifecycle

```text
(none) --submit_proposal--> Submitted --withdraw--> Withdrawn      (CAPSTONE-039)
                              |                       \--resubmit--> Submitted (rev n+1)
                              \--review/decision--> UnderReview -> Accepted | Rejected | ChangesRequested
```

Only `(none) -> Submitted` is in scope here. All other transitions are rejected
with `InvalidTransition`.

## Errors (stable codes)

| Code | Name | Action for caller |
| --- | --- | --- |
| 1 | `Unauthorized` | Caller lacks the participant role in the tenant |
| 2 | `TenantMismatch` | Call/proposal belongs to another tenant |
| 3 | `CallNotFound` | Check `call_id` |
| 4 | `CallClosed` | Deadline passed or opportunity closed |
| 5 | `TermsVersionStale` | Re-fetch terms and re-accept |
| 6 | `FormVersionStale` | Re-render the current form |
| 7 | `InvalidContentHash` | Hash must be non-zero SHA-256 |
| 8 | `ModeNotAllowed` | Pick an allowed delivery mode |
| 9 | `TeamNotEligible` | Team not formed or caller not a member |
| 10 | `DuplicateActiveSubmission` | Withdraw the existing one first |
| 11 | `IntentConflict` | Use a fresh `intent_id` |

## Events

`("CAPPROP", "submit")` → `(tenant_id, proposal_id, call_id, revision, form_version, terms_version)`.
No `content_hash` preimage, no submitter PII, no free text.

## Off-chain boundary

- Proposal body, abstract, methodology, budget narrative, attachments and
  reviewer-visible answers live in the proposals service; only `content_hash`
  reaches the chain.
- Form-field validation (word limits, attachment types) is an off-chain
  responsibility. The contract only enforces version, hash, and uniqueness.
- Eligibility evidence (enrolment, prerequisites) is attested off-chain. The
  contract checks a role/eligibility flag, not the evidence.

## Test plan

- Authorization tree: submitter auth required; team member vs non-member; foreign tenant.
- Replay: same `intent_id` + payload is idempotent (one record, one event); different payload gives `IntentConflict`.
- Concurrency: two submits for the same `(call, submitter)` in one ledger, where the second gets `DuplicateActiveSubmission`.
- Expiry: submit at `deadline` passes; at `deadline + 1` it fails with `CallClosed`.
- Stale versions: terms or form bumped between render and submit.
- Storage bounds: record size is constant; intent TTL extended and expired.
- Event schema snapshot; every error code asserted by number.
- Resource budget: `env.budget()` CPU/mem under the documented ceiling.
- Upgrade: records written by v1 decode after a WASM upgrade.

## Rollout / rollback

- Deploy `capstone-proposals` behind the off-chain feature flag. The proposals
  service dual-writes (DB first, then chain) and reconciles by `intent_id`.
- Rollback: disable the flag. On-chain records stay immutable and readable, and
  the service falls back to DB-only submissions with no data loss.

## Cross-repository work (CAPSTONE-038)

- **Backend:** canonical hashing of proposal content, intent-id generation, chain submission worker, reconciliation.
- **Frontend:** submit flow that shows frozen form/terms versions and handles `TermsVersionStale` / `FormVersionStale`.
