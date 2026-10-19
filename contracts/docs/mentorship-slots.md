# Mentorship Slots Contract

- Status: Implemented
- Owner: Mentorship On-chain working group
- Related issue: #1251 — `MENTOR-055` Find mutually available session slots
  (Scheduling track)
- Depends on / relates to: `contracts/mentorship-availability` (source of
  truth for participant windows, lead time, buffers, and horizon),
  `contracts/mentorship-calendar` (busy time enters availability as
  `Unavailable` exceptions), and, off-chain, the program catalogue

## Scope

`contracts/mentorship-slots` computes deterministic candidate session slots
for a mentor/mentee pair and books them with revalidation. It implements the
**On-chain** half of `MENTOR-055`:

- **Inputs.** A program policy (`set_program_policy`, admin-only) defines the
  session `duration_seconds`, the `granularity_seconds` grid, and
  `max_sessions_per_day`. Participant windows, lead time, buffers, and the
  booking horizon come from `contracts/mentorship-availability`, read through
  a cross-contract call.
- **Deterministic candidates.** `find_slots` intersects the two participants'
  materialized UTC ranges, aligns candidate starts to the granularity grid
  measured from the Unix epoch, and walks the intersections in order. The
  same inputs and ledger time always produce the same slots, independent of
  where a query starts or ends.
- **DST-safe.** All arithmetic is on absolute UTC instants. Local-time
  resolution and DST handling happen in the availability contract; slots
  never re-derive a time zone.
- **Capacity-aware.** Blackouts (per program or global), existing bookings
  including each participant's buffers, and the per-participant daily
  session cap are applied while generating candidates and again at booking.
- **Bounded and paginated.** A search covers at most 31 days, collects at most
  256 valid candidates, and examines a bounded number of instants; a range
  that is too dense or too constrained returns `TooManyCandidates` instead of
  running unbounded work. Returned slots are paged (at most 64 per page).
  Candidates are alternatives and may overlap each other; a page is ordered
  but not disjoint.
- **Stale candidates are revalidated at booking.** Every `Slot` echoes the
  availability versions it was computed from, and `book_slot` re-reads
  availability, blackouts, bookings, and capacity before writing. A slot that
  became invalid fails with `SlotUnavailable` (or `BlackoutOverlap`).
- **Idempotent booking.** Re-submitting the same booking id with the same
  pair, program, and start returns the existing end time without writing or
  emitting again. Reusing the id for a different slot is
  `BookingAlreadyExists`. Cancelling frees capacity immediately and fails
  clearly on a second attempt.

**Not implemented here:** payments/settlement for a session (the escrow and
payout contracts own funds), reminders and notifications
(`contracts/scholarship-*` communication patterns are the reference, and the
mentorship communications work items own the mentorship variant), and program
catalogue management (program ids are opaque references).

## Privacy

- Only opaque program ids, participant addresses, instants, and booking state
  are stored. Session topic, goals, notes, and any resource links stay
  off-chain.
- Blackouts carry a program id, not a reason. Availability exceptions carry
  only a reference hash.
- Booking records are public ledger data and reveal that two addresses met at
  a time. Programs that require stronger unlinkability should use
  pseudonymous participant addresses; the contract cannot provide that by
  itself.

## Ownership

- **Admin.** `initialize` sets the admin and the availability contract
  address; only the admin publishes program policies and blackouts and can
  repoint availability. All admin entry points compare the caller to the
  stored admin before requiring its signature.
- **Participants.** `book_slot` requires both the mentor's and the mentee's
  authorization (the caller must be one of them but is not separately
  authenticated — the two party signatures are the stronger condition).
  Either party can cancel. A non-party caller is `NotAuthorized`.
- Reads (`find_slots`, `is_slot_bookable`, booking and blackout lookups) are
  permissionless so the client can render availability without a session.

## Migration

New contract; no prior on-chain state. Deployment order matters:
1. Deploy `contracts/mentorship-availability` and register participant
   schedules.
2. Deploy this contract with `initialize(admin, availability_address)`.
3. Publish the program policy with `set_program_policy` before any search,
   otherwise `find_slots` returns `ProgramPolicyNotFound`.

Changing the availability contract address is an admin action
(`set_availability_contract`) and takes effect for the next call; existing
bookings are not migrated, because they are revalidated only at booking time.

## Operational impact

- **Storage/TTL.** Up to 64 blackouts in total, up to 128 bookings per
  participant, and one policy per program, all `persistent` with a ~1-year TTL
  refreshed on read and write.
- **Cross-contract cost.** Each search or booking performs bounded
  cross-contract reads: `get_schedule` for both participants, up to four pages
  of `available_ranges` per participant, and local storage reads. A missing or
  failing availability deployment surfaces as `AvailabilityUnavailable`
  rather than an opaque panic.
- **Events.** `PROGPOL`, `BLKSET`, `BLKREM`, `BOOKED`, `BOOKCAN`, and
  `AVAILSET` carry the ids, versions, and instants an indexer needs to build a
  booking timeline and to warm reminder jobs.
- **Rollout.** Ship behind the mentorship feature flag. Publish blackouts for
  holidays and cohort-wide events before enabling search; a blackout can be
  added at any time and immediately stops new bookings, but never changes
  existing ones. Expect `TooManyCandidates` for pathologically free schedules
  until the client narrows its range — that is the bounded-search contract,
  not an outage.
- **Failure codes.** Errors are stable and numbered
  (`ProgramPolicyNotFound = 5`, `InvalidSessionPolicy = 6`,
  `SlotUnavailable = 22`, `BlackoutOverlap = 23`,
  `AvailabilityUnavailable = 21`, …) so clients can distinguish "not
  bookable" from "misconfigured" from "stale".
- **Cross-repository work.** The matching `MENTOR-055` work in the other
  ChainVerse repositories owns the slot-picker UI, the program policy
  defaults, and the booking flow that calls `find_slots` and `book_slot`.
