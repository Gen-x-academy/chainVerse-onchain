# MENTOR-113: Mentorship Tenant Isolation Verification

**Track:** Privacy | **Component:** on-chain | **Status:** design

Goal: Add tenant isolation verification suite.

## On-chain

No new storage. Every storage key already includes the tenant id; the suite checks that this holds.

## Off-chain boundary

Off-chain services run the same checks against their own data.

## Rules

- Tests create two tenants and try every read and write across the boundary; each must fail.
- Covers program, profile, application, agreement, session, payment, report and analytics data.
- The suite runs on every pull request.

## Events and errors

Cross-tenant calls must fail with `NotAuthorized` and emit no event.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-113 work lives in the other ChainVerse repositories.
