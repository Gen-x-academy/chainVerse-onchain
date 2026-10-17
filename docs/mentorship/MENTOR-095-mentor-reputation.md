# MENTOR-095: Mentor Reputation Score

**Track:** Reputation | **Component:** on-chain | **Status:** design

Goal: Calculate robust mentor reputation.

## On-chain

Per mentor: counts of eligible reviews, completions and reliable sessions, and the latest score.

## Off-chain boundary

Review text and private case details stay off-chain.

## Rules

- Only verified reviews of completed engagements count.
- Below a minimum sample size the score is shown as not enough data.
- Each score can be explained from the stored counts.

## Events and errors

Event `score_updated(mentor)`. Errors: `NotAuthorized`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-095 work lives in the other ChainVerse repositories.
