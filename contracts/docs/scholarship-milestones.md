# Scholarship Milestones Contract

- Status: Implemented
- Owner: Scholarships On-chain working group
- Related issues: #1093 (milestone-based disbursement schedules)
- Depends on / relates to: ADR 0002 (`docs/adr/0002-scholarships-on-chain-boundaries.md`),
  `contracts/scholarship-awards` (awards reference a schedule by ID)

## Scope

This contract (`contracts/scholarship-milestones`) implements the
"Scholarships On-chain/Milestones" epic's behavior (#1093): split an award
into enrollment, attendance, coursework, completion, or custom verified
milestones, and release them in order once verified.

- **Percentages and amounts reconcile to the award.** Percentages are
  basis points and must sum to exactly 10,000. `Milestone` amounts are
  *derived* from those percentages against the schedule's `total_amount`;
  the final milestone absorbs the integer-division remainder, so the
  derived amounts always sum exactly to the award even when percentages do
  not divide evenly. Callers never supply amounts, which removes the
  possibility of a mismatched amount that doesn't reconcile.
- **Dates are ordered.** Milestone dates must be strictly increasing
  (`DatesNotOrdered` otherwise).
- **Immutable after activation except governed amendment.** A schedule is
  defined inactive (`define_schedule`), then activated exactly once
  (`activate_schedule`). While inactive it can only be defined once (a
  second define is `ScheduleAlreadyExists`). Once active, every change must
  go through `amend_schedule`, which is admin-only, bumps the schedule
  `version`, records the amend timestamp, and is refused once any milestone
  has been released (`ScheduleLockedAfterDisbursement`), so an amendment
  can never rewrite already-disbursed history.
- **Verification and ordered release.** `verify_milestone` marks a
  milestone verifiable; `release_milestone` releases it and enforces strict
  schedule order and one-release-only, so the schedule is a faithful,
  monotonic disbursement plan. `released_total` / `remaining_amount` report
  the authoritative progress.

**Not implemented here:** actually moving funds. Releasing a milestone
records the release and its amount; executing a transfer is follow-up work
and, per ADR 0002's disbursement non-goal, must be reviewed against the
existing payment contracts first.

## Privacy

On-chain storage never holds milestone labels or descriptions. A milestone
carries only a `label_hash: BytesN<32>` integrity commitment over its
off-chain text. Amounts, percentages, and dates are non-sensitive plan
metadata.

## Ownership

The admin (`initialize`) exclusively defines, activates, amends, verifies,
and releases. There is no recipient-facing entry point in this contract —
recipients interact with `scholarship-awards`, which references a schedule
by `award_id`. Verification is a trust decision made by the admin (or, in
future, a delegated verifier role); the contract records it but cannot
itself judge whether a milestone's real-world condition was met.

## Migration

New contract, no prior on-chain state. After deployment the admin must
`initialize`, then define a schedule keyed by the award ID it splits before
`scholarship-awards`' `create_award` can reference it. There is no
cross-contract validation that the referenced `award_id` exists in the
awards contract (see Operational impact).

## Operational impact

- **Storage/TTL.** One schedule record per award, `persistent` with a
  ~1-year TTL bump. Milestone count is bounded (`MAX_MILESTONES = 16`), so
  no record can grow without bound — consistent with ADR 0002's bounded-
  storage invariant.
- **Events.** `MSDEF`, `MSACT`, `MSAMD`, `MSVRF`, and `MSREL` carry the
  award ID (and milestone index where applicable) for off-chain indexing.
- **Cross-contract wiring.** This contract validates percentages, dates,
  and internal reconciliation against the `total_amount` supplied by the
  admin — it does not read the award's `amount` from
  `contracts/scholarship-awards`. An admin could therefore define a
  schedule whose `total_amount` differs from the award it references.
  Reconciling the two (e.g. the awards contract resolving a schedule's
  total) is follow-up integration, in line with ADR 0002's cross-contract
  non-goal. Off-chain tooling should assert `schedule.total_amount ==
  award.amount` before accepting a plan.
- **Verifier trust.** `verify_milestone` is admin-gated today; a future
  delegated verifier role (per ADR 0002's sponsor-org roles work) would
  replace this single-admin check without changing the release semantics.
