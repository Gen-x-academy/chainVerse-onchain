# MENTOR-093: Verified Mentor Reviews

**Track:** Reputation | **Component:** on-chain | **Status:** design

Goal: Create verified mentor review flow.

## On-chain

A review record: engagement id, rating per dimension, review text hash, moderation state and disclosure flag.

## Off-chain boundary

Optional review text stays off-chain.

## Rules

- Only the mentee of a completed engagement can review, once per engagement.
- Ratings use fixed structured dimensions.
- Moderation state decides whether a review is shown.

## Events and errors

Event `review_submitted(engagement)`. Errors: `NotEligible`, `AlreadyProcessed`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-093 work lives in the other ChainVerse repositories.
