# Scholarship Reviews Contract

- Status: Implemented
- Owner: Scholarships On-chain working group
- Related issues: #1086 (normalized aggregate scores)

## Scope

This contract (`contracts/scholarship-reviews`) implements #1086, combining
multiple independent reviews of an application into a single, reproducible
aggregate score:

- **Rubrics are versioned and immutable.** `publish_rubric()` records a
  rubric (an ordered list of `Criterion { weight_bps, max_score }`) as
  version N+1; existing versions are never overwritten, so the exact
  weighting that produced a past score stays recoverable via
  `get_rubric(program_id, version)`. A rubric is rejected (`InvalidRubric`)
  unless it is non-empty, has at most `MAX_CRITERIA` criteria, every
  `max_score` is positive, and the `weight_bps` values sum to exactly
  `BPS_DENOMINATOR` (10_000) — i.e. weights are basis points that must add
  up to 100%.
- **Scores are normalized per criterion.** `submit_review()` records a
  reviewer's raw scores only after checking there is exactly one score per
  criterion and every score is within that criterion's `max_score`
  (`InvalidScores` otherwise). The weighted total is then
  `sum(floor(score_i * 10_000 / max_score_i) * weight_i) / 10_000`, floored.
- **Aggregation is deterministic and transparent.** `aggregate_score()`
  divides the sum of the accepted reviews' weighted totals by the review
  count, floored at each division, and fails with `InsufficientReviews`
  until the program's configured `min_reviews` have been submitted.
- **Review slots are bounded.** `register_program()` sets `max_reviews`;
  a further review beyond that cap fails with `ReviewerCapacityExceeded`,
  and a reviewer may submit at most one review per application
  (`ReviewAlreadyExists`).

## Precision and rounding

Precision is documented and reproducible purely from immutable inputs:

1. Each criterion is normalized to basis points with a floored integer
   division: `normalized_i = floor(score_i * 10_000 / max_score_i)`, so
   `normalized_i` is in `[0, 10_000]`.
2. The reviewer's weighted total is
   `floor(sum(normalized_i * weight_i) / 10_000)`, again in
   `[0, 10_000]`.
3. The aggregate is `floor(sum(review_totals) / review_count)`.

Every division floors (never rounds to nearest), so the result is a pure
function of the recorded scores, the immutable rubric version they were
submitted against, and the number of reviews — and can be re-derived
off-chain from chain state alone. All score math uses checked arithmetic
and is bounded (`MAX_CRITERION_SCORE = 1_000`), so intermediate products
cannot overflow.

## Mixed rubric versions

Because rubrics are versioned, a review records the `rubric_version` it was
scored against. `aggregate_score()` reads that version from the reviews and
loads exactly that rubric; if the reviews were scored against different
rubric versions the call fails with `MixedRubricVersions` rather than
silently reinterpreting older scores under a newer weighting. To change
weights, publish a new rubric and collect a fresh, version-consistent set
of reviews.

## Tie policy

`compare_applicants()` ranks two applicants deterministically: the higher
`aggregate_score()` wins; if equal, the applicant whose earliest review was
submitted earlier wins; if still indistinguishable, it returns
`CompareOutcome::Tie`. The comparison depends only on recorded state, and
callers that need a total order can apply their own tie-break (e.g. a
randomized or policy draw) on top of an explicit `Tie`.

## Privacy

On-chain storage never holds review comments, evidence, or applicant
submissions. Only addresses, numeric scores, criterion weights/limits,
rubric versions, and timestamps are stored. Any narrative review content
stays off-chain; if it later needs to be commitmented, a `BytesN<32>` hash
can be threaded through without changing this contract's scoring surface.

## Ownership

The registering admin (`initialize`) is the only party able to register
programs, toggle their active state, and publish rubric versions.
Reviewers submit under their own signature (`submit_review` calls
`require_auth` on the reviewer), so no relayer can forge a review, and the
recorded `reviewer` is always the signer.

## Migration

This is a new contract with no prior on-chain state to migrate. The
program flow is: `initialize` → `register_program` → `publish_rubric` →
reviewers `submit_review` → `aggregate_score`/`compare_applicants`. A
program must have a published rubric before any review is accepted
(`NoRubricPublished`).

## Operational impact

- Storage: `ProgramConfig`, `Rubric`, `Review`, `RubricVersion` and
  `Reviewers` are `persistent` entries bumped to a ~1-year TTL on write,
  matching `course_registry`'s convention. Long-lived records should be
  periodically touched by an admin/indexer if they must outlive that
  window without archival restore.
- Events: `REVPROG` (program registered), `REVACT` (active flag changed),
  `RUBPUB` (rubric version published) and `REVIEW` (review recorded) are
  published for off-chain indexing and audit.
- Publishing a new rubric does not retroactively change existing reviews
  or scores; it only affects future submissions, and mixing versions in
  one aggregate is rejected explicitly.
- No upgrade/pause admin function exists in this pass.
