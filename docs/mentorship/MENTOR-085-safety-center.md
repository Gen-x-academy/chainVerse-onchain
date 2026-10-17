# MENTOR-085: Mentorship Safety Center

**Track:** Safety | **Component:** on-chain | **Status:** design

Goal: Create mentorship safety center.

## On-chain

A per-user pointer to their trusted-contact hash and the status of their open case.

## Off-chain boundary

Conduct rules, emergency guidance and blocking or reporting screens are served by the app.

## Rules

- Every participant can reach rules, privacy choices, blocking, reporting and emergency guidance.
- Case status shown to a user is limited to their own cases.
- Trusted contacts are stored as hashes only.

## Events and errors

Event `trusted_contact_set(user)`. Errors: `NotAuthorized`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-085 work lives in the other ChainVerse repositories.
