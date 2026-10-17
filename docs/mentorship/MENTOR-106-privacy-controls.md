# MENTOR-106: Mentorship Field-Level Privacy Controls

**Track:** Privacy | **Component:** on-chain | **Status:** design

Goal: Add field-level mentorship privacy controls.

## On-chain

A visibility setting per user and field (private, matched, public) for profile, goals, availability, achievements and contact fields.

## Off-chain boundary

The field values themselves stay off-chain; the contract only stores the visibility level.

## Rules

- Defaults are the most private level; a user can only loosen a field they own.
- Policy limits are enforced: some fields cannot be made public.
- Readers are checked against the setting at read time.

## Events and errors

Event `visibility_changed(user, field, level)`. Errors: `NotAuthorized`, `PolicyLimit`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-106 work lives in the other ChainVerse repositories.
