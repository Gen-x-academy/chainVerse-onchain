# MENTOR-057: Reschedule Mentorship Sessions

**Track:** Scheduling | **Component:** on-chain | **Status:** implemented (v1.1)

Goal: Reschedule mentorship sessions.

## On-chain

`contracts/mentorship-scheduling` extends session booking with a
proposal/approval reschedule flow:

- Either participant proposes a new start, duration and display offset for
  an existing session, with a blame-free `reason` code and a `reason_hash`
  commitment to the private explanation.
- The counterparty approves (applying the change) or declines it. A party
  cannot approve its own proposal.
- Changes are only accepted while the session is at least the configured
  notice period away; proposals expire after the approval window (capped
  at the original start), and approvals against a session that changed in
  the meantime fail as stale.
- The new slot is rechecked against both participants' sessions at
  proposal and at approval time.
- Every applied change appends a bounded `ScheduleChange` to the session
  and bumps `revision`; both confirmations reset so the new slot is
  reconfirmed.
- Each proposal previews the settlement impact (billable-minute delta and
  whether an adjustment is required).

## Off-chain boundary

Private explanations, calendar events, reminders and notification
delivery stay in the app. The chain stores the reason code, the
commitment, the times and the approval state. Rendering local time uses
the stored UTC start plus the bounded display offset. Payment adjustment
execution belongs to the settlement contracts; this contract only
previews it.

## Rules

- Only the session's mentor or mentee can propose.
- Only the non-proposing participant can approve or decline.
- The original start must be outside the notice window; admin-settable,
  bounded to 30 days.
- One open proposal per session; retrying the same proposal id and
  payload is a no-op, reusing it for a different payload fails.
- Approval window is bounded (60s–30 days) and can never outlive the
  meeting.
- At most 16 applied changes per session; history and storage are
  TTL-bounded.
- Applying emits one `RESCHED` event; proposing emits `RSPROP`; declining
  emits `RSDECL`.

## Events and errors

Events `RSPROP(proposal_id)`, `RESCHED(proposal_id)`,
`RSDECL(proposal_id)`, `RESPPOL`. Errors: `NotAuthorized`,
`RescheduleNotFound`, `RescheduleAlreadyPending`,
`RescheduleNoticeNotMet`, `RescheduleExpired`,
`StaleRescheduleApproval`, `InvalidReschedule`,
`RescheduleLimitReached`, `RescheduleNotOpen`,
`InvalidReschedulePolicy`, plus the existing booking errors
(`ScheduleConflict`, `InvalidDuration`, `InvalidTimezoneOffset`,
`LeadTimeNotMet`).

## Dependencies and rollout

Depends on session booking (MENTOR-056) in the same contract; ships as an
additive WASM upgrade under the same address. The settlement track
(MENTOR-070/071) consumes the previewed `PaymentImpact`; notifications
(MENTOR-102/103) and calendar projection consume `RSPROP`/`RESCHED`.
Configure the notice and approval windows before enabling the UI. The
matching MENTOR-057 work (reschedule UI, calendar updates, notification
copy, payment-adjustment flow) lives in the other ChainVerse repositories.
