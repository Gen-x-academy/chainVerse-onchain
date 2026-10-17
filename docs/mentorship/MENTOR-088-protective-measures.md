# MENTOR-088: Mentorship Protective Account Measures

**Track:** Safety | **Component:** on-chain | **Status:** design

Goal: Apply protective account measures.

## On-chain

A restriction record: subject hash, scope (discovery, messaging, booking, payout, contact), expiry ledger and case reference hash.

## Off-chain boundary

Case notes and evidence stay in the safety system.

## Rules

- Only an authorized responder may set or lift a restriction.
- Every restriction has a scope and an expiry; none is open-ended.
- Expired restrictions stop applying without any call.

## Events and errors

Events `restriction_set` and `restriction_lifted`. Errors: `NotAuthorized`, `NoExpiry`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-088 work lives in the other ChainVerse repositories.
