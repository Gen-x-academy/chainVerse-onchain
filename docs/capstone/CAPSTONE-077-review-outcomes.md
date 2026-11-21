# CAPSTONE-077: Review Outcome Aggregation

**Track:** Review | **Component:** on-chain | **Status:** implemented
**Issue:** #1425 | **Audit baseline:** `main` @ `5b62b369` (2026-11-18), revalidated against the current worktree

Goal: aggregate review outcomes fairly with explicit quorum, missing-review
handling, calibration policy, tie treatment, and no silent score mutation.

## Scope

`contracts/capstone-review-outcomes` stores an `Outcome` per
`(tenant_id, project_id, outcome_id)`. `record_score` accepts at most 64 unique
`review_id` contributions with `score` in `0..100` and `weight` in `1..100`.
`finalize` computes an integer score, applies the configured calibration, and
returns `Accepted`, `Rejected`, or `Escalated`. After finalization all score
writes fail with `AlreadyFinalized`; every versioned write uses
`expected_version`.

Revalidation against `main`: no aggregation primitive exists. This contract is
the settlement anchor for review outcomes and consumes scores recorded by
CAPSTONE-075.

## On-chain stores vs off-chain content

| On-chain | Type | Notes |
| --- | --- | --- |
| `Outcome(tenant_id, project_id, outcome_id)` | struct | Policies, quorum, counts, sums, min/max, final score, result, hashes, timestamps |
| `Contributions` / `Contribution(...)` | index + struct | Bounded review IDs with score, weight, timestamp |
| `Request(tenant_id, project_id, request_id)` | struct | Temporary receipt |
| `Role(subject, tenant_id, project_id, role)` | `bool` | Stored grants |

Off-chain: explanation narratives, per-reviewer rationale, calibration model
documentation, and the canonical report hashed as `explanation_hash` and
`policy_hash`. No text is stored on-chain.

## Authorization

Global admin grants tenant/project scoped `Coordinator` and `Auditor` roles.
Creation, score recording, and finalization call `require_auth()` and require a
stored `Admin` or `Coordinator` role. Auditors can read but cannot score.
Missing roles deny by default.

## Idempotency

`request_id` and `payload_hash` create a temporary receipt keyed by
`(tenant_id, project_id, request_id)`. Replays of `record_score` or `finalize`
return the original version/result without new writes or events. Different
payload hashes yield `RequestConflict`. Duplicate `review_id` contributions are
rejected with `DuplicateContribution`.

## Errors (stable, append-only codes)

| Code | Name |
| --- | --- |
| 1 | `AlreadyInitialized` |
| 2 | `NotInitialized` |
| 3 | `Unauthorized` |
| 4 | `NotFound` |
| 5 | `AlreadyExists` |
| 6 | `InvalidTransition` |
| 7 | `StaleVersion` |
| 8 | `VersionOverflow` |
| 9 | `RequestConflict` |
| 10 | `CollectionFull` |
| 11 | `InvalidScore` |
| 12 | `InvalidWeight` |
| 13 | `InvalidHash` |
| 14 | `ArithmeticOverflow` |
| 15 | `AlreadyFinalized` |
| 16 | `DuplicateContribution` |
| 17 | `QuorumNotMet` |
| 18 | `InvalidQuorum` |
| 19 | `MissingContributions` |

Codes are append-only and never renumbered.

## Aggregation rules

- `Exclude` — denominator is the sum of received weights; missing reviews do
  not affect the mean.
- `CountAsZero` — denominator is at least `quorum * 100`, so missing reviews
  lower the score without inventing contributions.
- `Block` — finalization fails with `QuorumNotMet` until the quorum is met.
- Calibration: `Disabled` keeps the raw score, `PriorAnchor` averages with 50,
  and `LinearRescale` maps the observed min/max span to `0..100`.
- Tie at exactly 50: `Reject`, `Escalate`, or `Accept` as configured.
- Score = `(weighted_sum * 100) / denominator` with checked arithmetic; all
  intermediate values are integers and stored for explainability.

## Events

`OCGRANT`, `OCRVOK` publish `(tenant_id, project_id, role)`. `OCNEW`, `OCSCOR`,
`OCFIN` publish IDs, versions, enum results, and no rationale or reviewer
identity.

## Privacy boundaries

Scores are numeric; explanations live off-chain behind `explanation_hash`. The
contract never stores comments, names, or reviewer rationale. Escalated ties are
an enum result only; the human escalation record remains in the workflow
service. Do not treat the public score as a full audit explanation.

## Dependencies

- `capstone-review-rubrics`: aggregation rule and weights.
- `capstone-independent-reviews`: `review_id` score evidence.
- `capstone-blinded-reviews`: projection gating for score visibility.
- `capstone-identifiers`: tenant/project/proposal scoping.

## Test plan

`Env::default()`, `mock_all_auths()`, generated clients, `try_*` assertions for
every code, replay and payload-conflict checks, stale versions, 64-contribution
bound, duplicate contributions, quorum policies, calibration outputs, tie
treatment, immutability after finalize, checked arithmetic paths, and timestamp
assertions.

## Rollout / rollback

Deploy behind a flag; the backend dual-writes outcomes and reconciles by
`request_id`. Rollback disables new outcomes while finalized records stay
immutable and readable. Schema and error changes are append-only so upgraded
contracts can read v1 records.

## Cross-repository work (CAPSTONE-077)

- **Backend:** aggregation job, explanation report generation, reconciliation,
  and escalation handoff.
- **Frontend:** explainable result view with score breakdown and tie badge.
- **Analytics:** fairness and calibration dashboards consuming stored enums and
  hashes only.
- **Ops:** deployment flag, rollback runbook, and audit export mapping.
