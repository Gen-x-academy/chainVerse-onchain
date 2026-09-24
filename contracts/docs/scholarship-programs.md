# Scholarship Programs Contract

- Status: Implemented (partial — see Scope)
- Owner: Scholarships On-chain working group
- Related issues: #1062 (application windows), #1064 (award inventory and
  budget ceilings), #1063 (cohort/academic-term scoping — not yet
  implemented), #1065 (versioned program terms — not yet implemented)

## Scope

This contract (`contracts/scholarship-programs`) implements two of the
"Scholarships On-chain/Programs" epic's behaviors:

- **#1062 — Application opening and deadline windows.** `set_program_window()`
  stores `opens_at`/`closes_at` as UTC ledger timestamps (the only
  representation the chain can compare deterministically),
  `timezone_offset_minutes` as display-only metadata for off-chain UIs, and
  a `grace_period_seconds` window after `closes_at` during which
  `is_within_grace()` returns true but `is_open()` does not — so a caller
  can distinguish "still accepting" from "late but within grace." Invalid
  ranges (`opens_at >= closes_at`) are rejected at write time. Changing a
  program's window later has no effect on anything already submitted
  elsewhere, because this contract holds no application records at all —
  see Migration/Operational notes.
- **#1064 — Award inventory and budget ceilings.** `configure_award_budget()`
  sets `max_recipients`, `per_award_amount`, and `total_budget`, rejecting
  any configuration where the stated budget couldn't even cover
  `max_recipients` awards at that rate. `reserve_award()`/`release_award()`
  atomically adjust `awarded_count`/`committed_amount`, and neither call can
  push either counter past its ceiling — every arithmetic step is
  `checked_*`. `remaining_capacity()` is always derived from these same
  counters, never estimated or cached elsewhere, so it's authoritative by
  construction.

**Not implemented here** (tracked separately): cohort/academic-term scoping
(#1063) — associating a program with one or more cohorts/terms/institutions
and validating overlap — and versioned program terms (#1065), which is the
same "publish an immutable version, tie prior records to the version they
accepted" pattern already implemented for form schemas and consent terms in
`scholarship-applications`, just applied to a program's broader terms
document; a natural follow-up reusing that pattern.

## Privacy

This contract stores no applicant-identifying data whatsoever — only
program-level configuration (window timing) and aggregate counters (award
counts and committed amounts). There is nothing to minimize here beyond
keeping it that way: no `Address` in this contract's storage is ever an
applicant, only the admin performing reservations.

## Ownership

The admin (`initialize`) exclusively controls window configuration, budget
configuration, and award reservation/release. There is currently no
separate "reservation operator" role distinct from admin — reserving an
award (e.g. as part of an award-decision workflow) requires the same admin
credential as configuring the budget in the first place.

## Migration

New contract, no prior on-chain state. `configure_award_budget` may be
called again for the same program before any award has been reserved to
replace its configuration; the acceptance criteria for "concurrent
decisions remain safe" is satisfied structurally, since every
`reserve_award`/`release_award` call is a single atomic read-check-write
within one Soroban invocation — there is no cross-call window where two
concurrent reservations could both observe stale remaining capacity.

## Operational impact

- This contract intentionally has no reference to `scholarship-applications`
  or any application record. That's what makes "deadline changes do not
  silently invalidate submitted applications" trivially true here — but it
  also means nothing in this contract automatically stops
  `scholarship-applications.submit_application()` from succeeding after a
  window closes elsewhere; wiring window/capacity enforcement into the
  submission flow itself is a follow-up integration, not something this
  pass does.
- `configure_award_budget` can currently be called again after reservations
  have already been made, which would silently reset `awarded_count`/
  `committed_amount` to 0 while real reservations still "exist" in the
  caller's mental model. Recommend adding an explicit guard (reject
  reconfiguration once `awarded_count > 0`, or require an explicit
  "reset" action) before this contract is used against a live budget.
- Storage/TTL/events follow the same conventions as the sibling scholarship
  contracts (persistent storage, ~1-year TTL bump on write).
