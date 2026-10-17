# MENTOR-087: Mentorship Safeguarding Case Triage

**Track:** Safety | **Component:** on-chain | **Status:** design

Goal: Triage safeguarding cases.

## On-chain

A case state (new, assigned, escalated, closed), severity level, assigned responder and due ledger.

## Off-chain boundary

Case details, age and jurisdiction policy checks, and responder rosters stay off-chain.

## Rules

- Severity decides the response timer; higher severity gets shorter timers.
- A case past its due ledger is escalated automatically on the next call.
- A conflicted responder cannot be assigned.

## Events and errors

Event `case_state(case, state)`. Errors: `InvalidTransition`, `NotAuthorized`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-087 work lives in the other ChainVerse repositories.
