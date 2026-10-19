# MENTOR-080: Release Per-session Mentor Payment

**Track:** Payments | **Component:** on-chain | **Status:** design

Goal: Release per-session mentor payment.

## On-chain

A settlement per session holding the released gross, platform fee, refund deduction, sponsor deduction and net amount to the mentor, together with the ledger time of release and the policy version applied.

## Off-chain boundary

The settlement receipt, the itemised breakdown shown to both parties and the payout notification are served by the app.

## Rules

- A session settles only once attendance is confirmed and the feedback or dispute window has closed. No open dispute may exist.
- Only escrow funds already marked available may be released. Settlement never draws on pending or unmatched funds.
- Gross, fee and deductions are computed with checked arithmetic. An overflow or underflow fails the settlement outright rather than releasing a partial amount.
- The policy version in force at settlement is snapshotted with the result, so a later policy change cannot alter what was already paid.
- Settlement is idempotent per session. A retry returns the original settlement and transfers nothing further.
- The released amount is derived from the accepted pricing model, never from a caller-supplied figure.
- Where sponsorship applies, the sponsor deduction is applied before the mentor amount and is visible as a separate line.

## Events and errors

Event `session_payment_released(session, gross, fee, net, policy_version)`. Errors: `NotAuthorized`, `SessionNotSettled`, `WindowOpen`, `DisputeOpen`, `FundsUnavailable`, `ArithmeticOverflow`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-080 work lives in the other ChainVerse repositories.