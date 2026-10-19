# MENTOR-058: Cancel Mentorship Sessions

**Track:** Scheduling | **Component:** on-chain | **Status:** design

Goal: Cancel mentorship sessions.

## On-chain

A cancellation record per booked session holding the cancelled status, the cancelling actor and their role, the policy version in force at the time, and the resulting refund and fee amounts.

## Off-chain boundary

The cancellation dialog, the participant-facing policy copy and the confirmation messages are served by the app.

## Rules

- Either booked participant may cancel for any reason; only a coordinator may cancel on behalf of both.
- The policy version is snapshotted at cancellation so a later policy edit cannot change a fee that has already been settled.
- Cancelling an already cancelled session returns the original result and emits no second settlement instruction.
- Capacity is released exactly once, and only once the cancellation is final.
- Notice inside the free window refunds in full; notice outside it applies the snapshotted cancellation fee.

## Events and errors

Event `session_cancelled(session, actor, reason, policy_version, refund, fee)`. Errors: `NotAuthorized`, `SessionNotCancellable`, `AlreadySettled`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-058 work lives in the other ChainVerse repositories.