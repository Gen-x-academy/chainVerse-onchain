# MENTOR-114: Mentorship Program Analytics

**Track:** Operations | **Component:** on-chain | **Status:** design

Goal: Create mentorship program analytics.

## On-chain

Aggregate counters per program and cohort (applications, matches, completions); no per-person data.

## Off-chain boundary

Wait time, retention and outcome reports are computed off-chain from the counters and events.

## Rules

- Counters are incremented by the contracts themselves, never by callers.
- Groups smaller than a minimum size are not reported.
- Only authorized program staff can read cohort counters.

## Events and errors

Event `counter_updated(program, cohort, metric)`. Errors: `NotAuthorized`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-114 work lives in the other ChainVerse repositories.
