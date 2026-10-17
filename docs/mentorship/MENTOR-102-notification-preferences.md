# MENTOR-102: Mentorship Notification Preferences

**Track:** Communication | **Component:** on-chain | **Status:** design

Goal: Add mentorship notification preferences.

## On-chain

A bitmask per user: allowed channels and categories (applications, proposals, sessions, tasks, payments, safety, marketing).

## Off-chain boundary

Channel addresses such as email or phone stay off-chain.

## Rules

- Marketing is off by default.
- Mandatory messages (safety and payment) cannot be turned off, and the user is told so.
- Only the user can change their own preferences.

## Events and errors

Event `preferences_changed(user)`. Errors: `NotAuthorized`, `MandatoryCategory`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-102 work lives in the other ChainVerse repositories.
