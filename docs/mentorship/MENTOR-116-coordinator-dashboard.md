# MENTOR-116: Coordinator Operations Dashboard

**Track:** Operations | **Component:** on-chain | **Status:** design

Goal: Build coordinator operations dashboard.

## On-chain

No new storage. The dashboard reads counters and states already emitted by the contracts.

## Off-chain boundary

Queues, capacity charts and intervention tools run in the web app.

## Rules

- Shows capacity, review queues, unmatched participants, expiring proposals, schedule risks, overdue milestones and disputes.
- Only authorized coordinators can see data for their own program.
- Intervention actions are recorded in the audit log.

## Events and errors

Reads use view functions; errors: `NotAuthorized`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-116 work lives in the other ChainVerse repositories.
