# Mentorship Availability Contract

- Status: Implemented
- Owner: Mentorship On-chain working group
- Related issue: #1249 — `MENTOR-053` Manage recurring mentor availability
  (Scheduling track)
- Depends on / relates to: `contracts/mentorship-slots` (consumes the
  materialized ranges and the schedule version) and, off-chain, the tz
  database that produces the UTC offset transitions

## Scope

`contracts/mentorship-availability` stores a participant-owned recurring
weekly schedule and materializes it into UTC ranges. It implements the
**On-chain** half of `MENTOR-053`:

- **Weekly windows.** `set_schedule` takes windows as local wall-clock
  weekday plus start/end minutes. Input must be ordered by
  (weekday, start minute) — anything else is `WindowsNotOrdered` — and
  overlapping or touching windows on the same weekday are **merged** into a
  canonical sorted list before storage, so normalization is deterministic and
  every reader sees the same schedule. Invalid shapes (`weekday > 6`,
  `start >= end`, `end > 1440`) fail with `InvalidWeeklyWindow`.
- **Time zone and DST.** A schedule carries a `timezone_hash` commitment over
  the off-chain IANA identifier, a base UTC offset, and up to
  `MAX_OFFSET_CHANGES` explicit offset transitions. This is how DST is
  handled: the off-chain scheduler resolves the tz database and publishes
  where the offset changes, and the contract converts local windows with the
  offset in effect at each instant. A local time in the spring-forward gap
  does not exist and produces no range; a local time that occurs twice (fall
  back) resolves to the **earliest** matching UTC instant. No locale rules are
  guessed on-chain, so a missing or wrong transition is a data error the
  caller can see and fix — not a silent shift.
- **Guard rails.** `effective_from` / `effective_until`, `lead_time_seconds`,
  per-participant `buffer_before_seconds` / `buffer_after_seconds`, and
  `booking_horizon_days` are validated against hard caps (`InvalidEffectiveRange`,
  `InvalidLeadTime`, `InvalidBuffer`, `InvalidBookingHorizon`) and clamp what
  `available_ranges` returns.
- **Exceptions.** `add_exception` records a bounded UTC range that either
  blacks out availability (`Unavailable`) or adds a one-off window
  (`Available`). The caller-supplied id makes submission idempotent
  (`ExceptionAlreadyExists` on a duplicate), `remove_exception` fails clearly
  when there is nothing to remove, and the count and span per exception are
  bounded (`TooManyExceptions`, `ExceptionSpanTooLarge`).
- **Materialization.** `available_ranges(participant, from, until, page,
  limit)` returns sorted, merged, non-overlapping UTC ranges clipped to the
  effective range, lead time, and booking horizon, with unavailable
  exceptions subtracted and available exceptions added. `is_available`
  answers the same question for a single interval.

**Not implemented here:** bookings, capacity, program policies, and slot
search (`contracts/mentorship-slots`), and calendar-provider imports
(`contracts/mentorship-calendar`) — one-off blocks from a calendar are
published here as `Unavailable` exceptions.

## Privacy

- The IANA time-zone name is stored only as `timezone_hash`; exception notes
  only as `reference_hash`.
- Weekly windows, offsets, and exception intervals are availability metadata,
  not content. No participant contact details, session topics, or notes are
  stored.
- Reading a schedule is permissionless (it is public availability data); every
  mutation requires the participant's own signature, so nobody can edit
  another participant's availability.

## Ownership

Each schedule and exception set is keyed by the participant address and can
only be written by that participant (`set_schedule`, `set_enabled`,
`add_exception`, `remove_exception` each require the participant's
authorization). There is no admin or back-office override, so availability
cannot be changed by the platform on a participant's behalf.

## Migration

New contract; no prior on-chain state. A participant without a schedule is
reported as `ScheduleNotFound`, which the scheduling contract surfaces as
`AvailabilityUnavailable`, so a missing schedule degrades to "no bookable
slots" rather than an unbounded default. Every mutation bumps the schedule
`version`, which `contracts/mentorship-slots` echoes on each candidate slot so
clients can detect staleness.

## Operational impact

- **Storage/TTL.** One schedule, one bounded exception id list, and one record
  per exception, all `persistent` with a ~1-year TTL refreshed on read and
  write. Weekly windows are capped at 28 before normalization, offset
  transitions at 8, and exceptions at 64, so no record grows without bound.
- **Query bounds.** One query covers at most 90 days and pages at most 64
  ranges; callers walk a longer span with repeated calls. Candidate
  generation walks at most one UTC day at a time, so contract CPU is bounded
  by the query span rather than by the schedule size.
- **Events.** `SCHSET`, `SCHENB`, `EXCSET`, and `EXCREM` carry the participant,
  version, and exception id for indexers.
- **Existing bookings are never silently changed.** This contract holds no
  bookings. Changing a schedule bumps the version and takes effect for future
  materializations; sessions already booked in `contracts/mentorship-slots`
  are untouched, and re-booking revalidates against the new schedule.
- **Rollout.** Publish offset transitions ahead of each DST boundary (two per
  year in most zones) so materialized ranges stay exact; a missing transition
  shows up as an offset error in the affected window, not as silently shifted
  slots. Ship behind the mentorship feature flag.
- **Cross-repository work.** The matching `MENTOR-053` work in the other
  ChainVerse repositories owns the tz-database lookup that produces
  `offset_changes`, the availability UI, and the publication of one-off
  exceptions.
