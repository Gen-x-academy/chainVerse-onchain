# MENTOR-109: Mentorship Erasure Workflow

**Track:** Privacy | **Component:** on-chain | **Status:** design

Goal: Implement mentorship erasure workflow.

## On-chain

An erasure request record: subject hash, state (requested, in progress, done, partly retained) and the hash of the retention reason.

## Off-chain boundary

Profiles, drafts, messages, files, analytics and derived indexes are deleted or anonymized off-chain; each system reports back.

## Rules

- Financial and safety records may be retained for lawful reasons, and the reason is recorded.
- A request moves forward only through allowed states and is idempotent.
- Only the subject or an authorized operator may open or close a request.

## Events and errors

Event `erasure_state(subject_hash, state)`. Errors: `InvalidTransition`, `NotAuthorized`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-109 work lives in the other ChainVerse repositories.
