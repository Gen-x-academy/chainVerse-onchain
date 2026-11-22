# CAPSTONE-075: Independent Proposal Reviews

**Track:** Review | **Component:** on-chain | **Status:** implemented
**Issue:** #1423 | **Audit baseline:** `main` @ `5b62b369` (2026-11-18), revalidated against the current worktree

Goal: record independent reviews with criterion scores, comments commitment,
confidence, recommendation, conflict declarations, and immutable linkage to a
specific proposal submission version.

## Scope

`contracts/capstone-independent-reviews` stores one review per `review_id` per
`(tenant_id, project_id)`. A review is `Submitted` or `Withdrawn`.
`submit_review` freezes `proposal_id`, `submission_version`, `rubric_id`,
`rubric_version`, and `reviewer_ref`. `amend_review` and `withdraw_review`
require `expected_version`; a withdrawn review cannot be amended. Scores are
bounded (1..16 entries, each `0..100`) and per-submission review IDs are bounded
to 64.

Revalidation against `main`: no independent-review registry exists. This is the
on-chain evidence anchor that outcome aggregation reads.

## On-chain stores vs off-chain content

| On-chain | Type | Notes |
| --- | --- | --- |
| `Review(tenant_id, project_id, review_id)` | struct | Frozen linkage, enums, status, version, score count, timestamps, `reviewer` address, `reviewer_ref` |
| `Scores(tenant_id, project_id, review_id)` | `Vec<Score>` | Bounded criterion IDs and numeric points |
| `SubmissionIndex(tenant_id, project_id, proposal_id, submission_version)` | `Vec<BytesN<32>>` | Bounded review ID index, max 64 |
| `Request(tenant_id, project_id, request_id)` | struct | Temporary receipt |
| `Role(subject, tenant_id, project_id, role)` | `bool` | Stored grants |

Off-chain: comment bodies, quotations, reviewer notes, attachments, and the
canonical document hashed as `comments_hash`; the participant identity mapping
behind `reviewer_ref`. The ledger holds only the hash and the opaque reference.

## Authorization

Global admin bootstraps tenant/project scoped `Reviewer`, `Coordinator`, and
`Auditor` roles. All mutating calls invoke `require_auth()` and then require a
stored `Admin` or `Reviewer` role. `amend_review` and `withdraw_review`
additionally require that the caller is the stored original reviewer. Missing
roles deny by default.

## Idempotency

`request_id` plus `payload_hash` yields a temporary receipt keyed by
`(tenant_id, project_id, request_id)`. Replays return the original version
without new writes or events; a different payload hash yields
`RequestConflict`. Submission and version checks happen after the replay check,
so retries are stable.

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
| 12 | `InvalidHash` |
| 13 | `InvalidVersion` |
| 14 | `ArithmeticOverflow` |
| 15 | `InvalidReviewerRef` |

Codes are append-only and never renumbered.

## Events

`RVGRANT`, `RVRVOK` publish `(tenant_id, project_id, role)`. `RVNEW`, `RVAMD`,
`RVWDR` publish `(tenant_id, project_id, review_id, version)`. No comments,
scores, or reviewer identity are emitted.

## Privacy boundaries

Comments are never stored on-chain; only `comments_hash`. `reviewer_ref` is an
opaque pseudonym used by downstream queues and exports. The stored reviewer
address is an authorization identifier, not profile data; it is not emitted in
events. For identity hiding use CAPSTONE-076 mode rather than publishing raw
review text here.

## Dependencies

- `capstone-review-rubrics`: frozen `rubric_id` / `rubric_version`.
- `capstone-identifiers`: `proposal_id` / submission version linkage.
- `capstone-blinded-reviews`: supplies `reviewer_ref` for blinded assignments.
- `capstone-review-outcomes`: consumes scores via `review_id`.

## Test plan

`Env::default()` and `mock_all_auths()` fixtures, generated clients, `try_*`
error assertions, role-scope trees, submission idempotency and payload
conflicts, immutable linkage, versioned amendment/withdrawal, stale versions,
16-score and 64-review bounds, non-zero hashes, and timestamp checks.

## Rollout / rollback

Flag-gated deployment with DB-first dual writes keyed by `request_id`.
Rollback disables chain writes; existing review records remain readable and
immutable. Future versions add codes/fields without rewriting stored records.

## Cross-repository work (CAPSTONE-075)

- **Backend:** comment hashing, reviewer assignment feed, reconciliation of
  `StaleVersion` and `RequestConflict`.
- **Frontend:** review form with recommendation/confidence/conflict controls and
  retry-safe submission.
- **Moderation:** conflict declaration workflow and withdrawal SLA.
