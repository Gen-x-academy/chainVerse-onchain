# CAPSTONE-039: Proposal Withdrawal and Resubmission

**Track:** Proposals | **Component:** on-chain | **Status:** specified (not yet implemented)
**Issue:** #1387 | **Audit baseline:** `main` @ `5b62b369` (2026-11-18), revalidated against `main` @ `4471755`
**Depends on:** CAPSTONE-038 (`capstone-proposals`)

Goal: allow withdrawal before configured cutoffs, and linked resubmission only
when policy permits, without copying stale consent or decisions.

## On-chain scope

Extends `contracts/capstone-proposals` with:

```text
withdraw_proposal(submitter, tenant_id, proposal_id, expected_revision: u32, reason_code: u32, intent_id)
resubmit_proposal(submitter, tenant_id, prior_proposal_id, new_proposal_id,
                  form_version, terms_version, content_hash, intent_id) -> u32 /* revision */
set_withdrawal_policy(admin, tenant_id, call_id, policy: WithdrawalPolicy)
```

`WithdrawalPolicy` is stored per `(tenant_id, call_id)`:

| Field | Type | Meaning |
| --- | --- | --- |
| `withdraw_cutoff` | `u64` | Last timestamp a withdrawal is accepted |
| `allow_resubmit` | `bool` | Whether resubmission is permitted at all |
| `resubmit_cutoff` | `u64` | Last timestamp for resubmission (`<=` call deadline) |
| `max_revisions` | `u32` | Upper bound on the revision chain (bounded storage) |

## Rules

- **Withdraw.** Only the original submitter (or the team initiator, for team
  proposals) may withdraw. This is allowed from `Submitted` or
  `ChangesRequested`, and only while `now <= withdraw_cutoff`. A proposal in
  `UnderReview` (after cutoff), `Accepted` or `Rejected` cannot be withdrawn
  (`InvalidTransition`).
- **Optimistic concurrency.** `expected_revision` must match the stored
  revision, otherwise `StaleRevision`. This prevents a withdraw from racing a
  reviewer decision or a resubmission.
- **Reason codes** are a closed enum (`ChangedPlans`, `TeamChange`, `Error`,
  `Other`). Free-text reasons stay off-chain.
- **Resubmit** creates a new `Submission` with `revision = prior.revision + 1`
  and `prior = prior_proposal_id`. It requires:
  - the prior is `Withdrawn` or `ChangesRequested`;
  - `allow_resubmit` is true, `now <= resubmit_cutoff`, and `revision <= max_revisions`;
  - the full CAPSTONE-038 validation runs again against **current** terms and
    form versions.
- **No stale carry-over.** Resubmission never copies `terms_version`, consent
  acceptance, reviewer assignments, scores or decisions from the prior record.
  Consent is re-captured through the fresh `terms_version`. Assignments and
  decisions start empty.
- The prior record becomes `Superseded`, so one active submission per
  `(call, submitter)` still holds.
- Replays: the same `intent_id` with the same payload returns the original
  result and emits no second event.

## Lifecycle additions

```text
Submitted | ChangesRequested --withdraw (<= cutoff)--> Withdrawn
Withdrawn | ChangesRequested --resubmit (policy)-----> Superseded  +  new Submitted(rev+1, prior=…)
```

## Errors (appended to CAPSTONE-038 codes)

| Code | Name | Action |
| --- | --- | --- |
| 12 | `WithdrawalCutoffPassed` | Contact the coordinator off-chain |
| 13 | `ResubmissionNotAllowed` | Policy disallows resubmission |
| 14 | `ResubmissionCutoffPassed` | |
| 15 | `MaxRevisionsReached` | |
| 16 | `StaleRevision` | Reload proposal and retry |
| 17 | `InvalidTransition` | Proposal state does not permit the action |
| 18 | `PolicyNotFound` | Coordinator must configure the policy |

## Events

- `("CAPPROP", "withdraw")` → `(tenant_id, proposal_id, revision, reason_code)`
- `("CAPPROP", "resubmit")` → `(tenant_id, new_proposal_id, prior_proposal_id, revision)`

## Off-chain boundary

Withdrawal explanations, coordinator correspondence and the content diff
between revisions stay in the proposals service. Only the `content_hash` of
each revision is on-chain. Retention or deletion of the withdrawn content is
handled off-chain, and the chain keeps only the commitment.

## Test plan

- Auth: submitter, team initiator, non-member, foreign tenant, admin-only policy setter.
- Transitions: every allowed and disallowed `from → to` pair.
- Cutoffs: exact cutoff passes and `+1` fails, for both withdraw and resubmit.
- Race: reviewer decision increments the revision, then withdraw with the old `expected_revision` gets `StaleRevision`.
- No carry-over: resubmission has empty assignments and decisions, and a stale `terms_version` is rejected.
- Bounds: the chain stops at `max_revisions`.
- Replay idempotency, event schema snapshots, error codes, budget, upgrade decode.

## Rollout / rollback

Ships after CAPSTONE-038. Policies default to "withdraw disabled" until a
coordinator sets one, so deploying is a no-op for existing calls. Rollback:
remove the policy, which blocks new withdrawals. Existing `Withdrawn` and
`Superseded` records remain readable.

## Cross-repository work (CAPSTONE-039)

- **Backend:** policy admin, withdrawal reasons storage, revision diffing, chain worker.
- **Frontend:** withdraw/resubmit actions gated on cutoffs, with a revision chain view.
