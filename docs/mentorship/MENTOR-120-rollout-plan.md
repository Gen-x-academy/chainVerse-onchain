# MENTOR-120: Mentorship Rollout and Migration Plan

**Track:** Operations | **Component:** on-chain | **Status:** design

Goal: Publish mentorship rollout and migration plan.

## On-chain

A feature-flag value per stage and a contract version number; no user data.

## Off-chain boundary

Pilot tenant lists, data migrations, monitoring and support runbooks are kept in operations documents.

## Rules

- Phases: internal, pilot tenants, limited availability, general availability.
- Each phase has entry checks, monitoring and rollback triggers; any trigger returns to the previous phase.
- Migrations are backward compatible for one release.

## Events and errors

Event `stage_changed(stage)`. Errors: `NotAuthorized`, `InvalidStage`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-120 work lives in the other ChainVerse repositories.
