# Scholarship Decisions Contract

- Status: Implemented
- Owner: Scholarships On-chain working group
- Related issues: #1087 (committee decision workflow), #1088 (reserve budget
  during award decisions), #1089 (applicant appeals)

## Scope

This contract (`contracts/scholarship-decisions`) implements three of the
"Scholarships On-chain/Decisions" epic's behaviors:

- **#1087 — Committee decision workflow.** A chair opens a decision for an
  application (`open_decision`), active members stage it with
  award/waitlist/reject votes (`vote`) or withdraw (`recuse`), and a
  quorum of distinct *non-recused* votes is required before the decision is
  finalized (`finalize_decision`). Roles are enforced (`CommitteeRole::
  { Reviewer, Chair }`), every decision references the evidence version/hash
  it was taken on, and every open/vote/recuse/finalize action is recorded
  in the decision's bounded vote list and emitted as an event, so the whole
  trail is auditable.
- **#1088 — Reserve budget during award decisions.** When a decision
  finalizes as `Awarded`, the program's `per_award_amount` is held in a
  `Reservation` *before* the applicant accepts. Reservations are created
  atomically, can never push `reserved + awarded` past `total_budget`,
  expire after the program's `reservation_ttl`, and free their capacity
  exactly once.
- **#1089 — Applicant appeals.** An affected applicant may file one appeal
  (`file_appeal`) against a finalized rejected/waitlisted decision, within
  the program's `appeal_window`, carrying `grounds_hash`/`evidence_hash`
  commitments over off-chain content. Members who voted on the original
  decision are excluded from the appeal vote
  (`AppealReviewerIneligible`); the appeal is stored separately from the
  decision it challenges; and a resolved appeal supersedes the decision's
  outcome and is final (it bumps `outcome_version` and no second appeal may
  be filed).

## Quorum and roles

- `add_member`/`remove_member` (admin-only) manage the committee; a
  removed member stops counting toward quorum and cannot vote, but their
  role record is retained in case they are re-added. `MAX_MEMBERS` (32)
  bounds committee size.
- `set_quorum` (admin-only) sets the number of distinct non-recused votes
  needed to finalize a decision or appeal; it must be in `1..=MAX_MEMBERS`
  (`InvalidQuorum`).
- Only an active `Chair` may `open_decision`; `finalize_decision` and
  `finalize_appeal` may be called by an active chair or the admin
  (`NotAuthorized` otherwise); any active member may `vote`/`vote_appeal`.
  Every state-changing entry point calls `require_auth` on its caller.

## Tie policy

The outcome is chosen by a documented, deterministic policy: the
most-supported of award/waitlist/reject wins; ties resolve in favor of the
most conservative outcome in the order `Rejected` > `Waitlisted` >
`Awarded`. Recusals are excluded from the tally entirely, so they never
count toward quorum or toward any outcome.

## Finality

- A finalized decision stops accepting votes
  (`DecisionAlreadyFinalized`). An appeal keeps the decision's
  `outcome_version` in step: the original finalization records version 1,
  and a resolved appeal increments it, so the current outcome is always
  traceable back to the evidence version and vote record that produced it.
- An outcome is appealable only when it was finalized and is `Rejected` or
  `Waitlisted` (`DecisionNotFinalized`, `DecisionNotAppealable`), only
  within `finalized_at + appeal_window` (`AppealWindowClosed`), and only
  once per decision (`AppealAlreadyExists`) — including after the appeal
  resolves, so an appeal outcome is final.

## Reservations

- `register_program` (admin-only) sets the program's `total_budget`,
  `per_award_amount`, `reservation_ttl` and `appeal_window`; all must be
  positive and the total must cover at least one award
  (`InvalidProgramConfig`).
- `reserve_budget` runs inside `finalize_decision` (and `finalize_appeal`
  when the appeal awards) *before* the decision is marked finalized. It
  checks `reserved_amount + awarded_amount + amount <= total_budget` and
  fails with `BudgetExceeded` rather than over-committing; a failed
  reservation leaves the decision open.
- The reservation expires at `created_at + reservation_ttl`. The applicant
  can `accept_award` before expiry, converting the hold into
  `awarded_amount`; after expiry the accept fails with
  `ReservationExpired`, and a chair/admin can `release_reservation` early.
  `expire_reservation` is permissionless and may be called by anyone once
  the TTL has passed (`ReservationNotExpired` before then). Claiming,
  releasing or expiring a reservation changes its status away from
  `Reserved`, so any repeat attempt fails with `ReservationNotActive` —
  capacity is freed exactly once.

## Privacy

On-chain state never holds applications, review content, evidence bundles,
appeal grounds, or vote rationales. Everything sensitive is represented
on-chain only as a caller-supplied `BytesN<32>` commitment
(`evidence_hash`, `grounds_hash`). Records contain addresses, enum choices,
numeric versions, amounts, and timestamps — no PII.

## Ownership

The admin (`initialize`) manages membership, quorum and program
configuration. Chairs open and finalize decisions; members vote under their
own signature. Applicants sign `accept_award` and `file_appeal` themselves,
so no admin or relayer can accept on an applicant's behalf or file an
appeal for them.

## Migration

This is a new contract with no prior on-chain state to migrate. It is
designed to sit alongside the other scholarship contracts: decision
`evidence_version`/`evidence_hash` can reference the reviews contract's
(#1086) rubric/aggregate state, and award reservations are self-contained
here. Deployment flow: `initialize` → `add_member`/`set_quorum` →
`register_program` → decisions.

## Operational impact

- Storage: `ProgramConfig`, `Decision`, `Reservation`, `Appeal`,
  `MemberRole`, `Member` and `MemberList` are `persistent` entries bumped to
  a ~1-year TTL on write, matching `course_registry`'s convention. A
  long-lived committee list or decision record should be periodically
  touched by an admin/indexer if it must outlive that window.
- Events: `CMEMBR`, `CQUORUM`, `PROGREG`, `DECOPEN`, `DECVOTE`, `DECFIN`,
  `DECRSV`, `AWACC`, `RSVREL`, `RSVEXP`, `APPEAL`, `APVOTE` and `APFIN` are
  published for off-chain indexing and audit.
- Expiring a reservation returns capacity to the pool; if an award is later
  re-decided it must reserve again. Expiring does not rewrite the
  decision's outcome, so operators/indexers should reconcile
  reservation-expiry events against award state.
- No upgrade/pause admin function exists in this pass.
