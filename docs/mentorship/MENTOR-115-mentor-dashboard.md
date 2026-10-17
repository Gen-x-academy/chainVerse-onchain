# MENTOR-115: Mentor Operations Dashboard

**Track:** Operations | **Component:** on-chain | **Status:** design

Goal: Build mentor operations dashboard.

## On-chain

No new storage. The dashboard reads states and amounts from existing contracts.

## Off-chain boundary

Task lists, availability gaps and alert delivery run in the web app.

## Rules

- Shows upcoming sessions, applications, active mentees, tasks, milestones, availability gaps, earnings and alerts.
- A mentor sees only their own mentees and earnings.
- Earnings come from settled payments only.

## Events and errors

Reads use view functions; errors: `NotAuthorized`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-115 work lives in the other ChainVerse repositories.
