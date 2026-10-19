# MENTOR-081: Release Milestone-based Payment

**Track:** Payments | **Component:** on-chain | **Status:** design

Goal: Release milestone-based payment.

## On-chain

A settlement per milestone holding the milestone and deliverable it settles, the amount, the challenge window close time and the ledger time of release.

## Off-chain boundary

The milestone payout receipt, the acceptance screen and the dispute prompt shown during the challenge window are served by the app.

## Rules

- A milestone pays out only after its designated deliverable submission has been accepted and the challenge window has closed.
- Acceptance must be by the designated reviewer. A submitter cannot accept their own work and trigger a payout.
- The challenge window is recorded when acceptance happens, not when submission happened, so the window always runs from the accepted version.
- The released amount comes from the milestone's payment link into the agreement, never from a caller-supplied figure.
- Settlement is idempotent per milestone. A retry returns the original settlement and transfers nothing further.
- An opened dispute blocks release for the whole challenge window. Release is refused rather than deferred automatically.
- A milestone with no payment link releases nothing and is marked as such rather than failing.

## Events and errors

Events `milestone_accepted(milestone, deliverable, challenge_closes)` and `milestone_payment_released(milestone, amount)`. Errors: `NotAuthorized`, `DeliverableNotAccepted`, `ChallengeWindowOpen`, `DisputeOpen`, `MilestoneAlreadySettled`, `ArithmeticOverflow`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-081 work lives in the other ChainVerse repositories.