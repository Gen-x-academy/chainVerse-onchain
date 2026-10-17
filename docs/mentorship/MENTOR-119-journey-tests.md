# MENTOR-119: Mentorship End-to-End Journey Tests

**Track:** Operations | **Component:** on-chain | **Status:** design

Goal: Add end-to-end mentorship journey tests.

## On-chain

No new storage. The tests drive the existing contracts through the full journey.

## Off-chain boundary

Off-chain steps (discovery, application, session delivery, review) are simulated with fixed inputs.

## Rules

- Journey: organization setup, mentor publication, application, match, agreement, funding, sessions, milestones, completion, review, credential check.
- Every step asserts the resulting state and emitted event.
- Tests are deterministic and use fixed ledger times.

## Events and errors

A failing step reports the step name and the stable error code.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-119 work lives in the other ChainVerse repositories.
