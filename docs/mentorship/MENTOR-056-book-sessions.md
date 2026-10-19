# MENTOR-056: Book Mentorship Sessions

**Track:** Scheduling | **Component:** on-chain | **Status:** implemented (v1)

Goal: Book mentorship sessions.

## On-chain

Session records anchored to an active agreement: participants, UTC start
plus a bounded local display offset, duration, delivery mode, and
commitments to the off-chain meeting reference and agenda. Booking
enforces agreement participation, minimum lead time, duration bounds,
and rejects any slot overlapping an existing session of the mentor or
mentee. Both participants confirm before a session becomes confirmed.

## Off-chain boundary

Meeting links, venues, agenda text, calendars and notifications are held
by the app; the chain stores only commitments, times and status. The
registrar role that registers agreement anchors moves to the canonical
agreements contract (MENTOR-045/046) when it ships.

## Rules

- Only the mentor and mentee of an active agreement can book under it.
- A slot that overlaps an existing session of either participant is
  rejected; back-to-back sessions are allowed.
- Retrying a booking with the same idempotency key and payload returns
  the original session id; reusing the key for a different payload
  fails.
- Per-participant session index is capped; storage is TTL-bounded.
- Booking emits one canonical `BOOKED` event; confirmations emit `CFMD`.

## Events and errors

Events `BOOKED(session_id)`, `CFMD(session_id)`, `AGREG(agreement_id)`,
`AGACT(agreement_id, active)`. Errors: `NotAuthorized`,
`AgreementNotActive`, `ScheduleConflict`, `LeadTimeNotMet`,
`StartTimeInPast`, `InvalidDuration`, `InvalidTimezoneOffset`,
`SessionLimitReached`, `IdempotencyConflict`.

## Dependencies and rollout

Deploy before the reschedule increment (MENTOR-057), which extends the
same contract additively. Cancellation (MENTOR-058), no-shows
(MENTOR-059) and completion build on the same session state. The
matching MENTOR-056 work lives in the other ChainVerse repositories.
