# MENTOR-094: Mentor Responses to Reviews

**Track:** Reputation | **Component:** on-chain | **Status:** design

Goal: Allow mentor responses to reviews.

## On-chain

A response hash per review and a moderation state (visible, requested, hidden).

## Off-chain boundary

Response text is stored off-chain and must not reveal private session or case details.

## Rules

- Only the reviewed mentor can respond, and only once per review.
- The mentor can request moderation of a review.
- A hidden response keeps its hash for the record.

## Events and errors

Events `response_posted` and `moderation_requested`. Errors: `NotAuthorized`, `AlreadyProcessed`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-094 work lives in the other ChainVerse repositories.
