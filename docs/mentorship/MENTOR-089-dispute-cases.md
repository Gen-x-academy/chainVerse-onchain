# MENTOR-089: Mentorship Dispute Cases

**Track:** Safety | **Component:** on-chain | **Status:** design

Goal: Implement mentorship dispute cases.

## On-chain

A dispute record: subject (session, deliverable, refund or settlement), state, claim hash, response hash, evidence hash list and deadlines.

## Off-chain boundary

Claims, responses and evidence files stay off-chain.

## Rules

- States: opened, responded, in review, resolved; only forward moves are allowed.
- Each party can add one claim or response before its deadline.
- A dispute freezes the related payment until it is resolved.

## Events and errors

Events `dispute_opened` and `dispute_state`. Errors: `InvalidTransition`, `DeadlinePassed`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-089 work lives in the other ChainVerse repositories.
