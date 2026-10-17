# MENTOR-104: Mentorship Reminder Scheduling

**Track:** Communication | **Component:** on-chain | **Status:** design

Goal: Create reminder scheduling engine.

## On-chain

A reminder record: owner hash, kind, due ledger and state (scheduled, sent, cancelled).

## Off-chain boundary

Sending the reminder and its text are done by the off-chain worker.

## Rules

- Kinds: application deadline, proposal expiry, session, task, milestone, review, payment action.
- A reminder can be cancelled until it is sent; cancelling twice is harmless.
- Only the owner or the scheduler address may create or cancel a reminder.

## Events and errors

Events `reminder_scheduled` and `reminder_cancelled`. Errors: `NotAuthorized`, `PastDue`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-104 work lives in the other ChainVerse repositories.
