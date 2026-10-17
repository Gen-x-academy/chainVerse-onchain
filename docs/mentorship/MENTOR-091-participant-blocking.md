# MENTOR-091: Mentorship Participant Blocking

**Track:** Safety | **Component:** on-chain | **Status:** design

Goal: Add bidirectional participant blocking.

## On-chain

A block entry per pair of users, stored under a canonical order so it applies in both directions.

## Off-chain boundary

Existing safety and financial records are kept off-chain and are not changed by a block.

## Rules

- A block stops future discovery, applications, matching, messaging and booking between the two users.
- Only one of the two users can create or remove the block.
- Blocking twice has no extra effect.

## Events and errors

Events `blocked` and `unblocked`. Errors: `NotAuthorized`, `SelfBlock`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-091 work lives in the other ChainVerse repositories.
