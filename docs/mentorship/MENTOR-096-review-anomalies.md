# MENTOR-096: Suspicious Review Activity Flags

**Track:** Reputation | **Component:** on-chain | **Status:** design

Goal: Detect suspicious review activity.

## On-chain

A flag per review: pattern code (reciprocal, burst, duplicate, coerced, other) and review state (flagged, cleared, upheld).

## Off-chain boundary

Pattern detection runs off-chain; only the flag and the reviewer's decision are recorded.

## Rules

- A flag never changes a review by itself; a human reviewer decides.
- Flags use neutral wording and do not name a person as guilty.
- Only authorized reviewers can clear or uphold a flag.

## Events and errors

Event `review_flagged(review, pattern)`. Errors: `NotAuthorized`, `InvalidTransition`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-096 work lives in the other ChainVerse repositories.
