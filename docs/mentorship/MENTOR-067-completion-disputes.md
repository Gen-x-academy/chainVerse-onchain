# MENTOR-067: Support Session Completion Disputes

**Track:** Sessions | **Component:** on-chain | **Status:** design

Goal: Support session completion disputes.

## On-chain

A dispute per session and claimant holding the contested aspect, the ledger time it was raised, the response deadline, the evidence hash references and the resolution state.

## Off-chain boundary

Evidence upload, the coordinator review screen and the parties' written submissions are served by the app.

## Rules

- A dispute must be raised before settlement and within the configured window after the session.
- Contestable aspects are a closed set: attendance, completion, duration and outcome.
- Only a booked participant may raise a dispute, and only against a session they were party to.
- Opening a dispute pauses settlement for that session until the resolution state is set.
- The respondent gets the full response window; silence resolves in favour of the existing record.
- A second dispute on the same aspect merges into the first.
- Resolution records who decided and under which policy version; it never overwrites the original contested record.

## Events and errors

Events `session_dispute_opened(session, claimant, aspect)` and `session_dispute_resolved(session, aspect, outcome)`. Errors: `NotAuthorized`, `NotBooked`, `SettlementClosed`, `WindowClosed`, `DisputeExists`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-067 work lives in the other ChainVerse repositories.