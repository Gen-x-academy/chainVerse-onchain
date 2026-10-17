# MENTOR-108: Mentorship Data Export

**Track:** Privacy | **Component:** on-chain | **Status:** design

Goal: Provide mentorship data export.

## On-chain

An export receipt: requester hash, export hash, manifest hash and ledger time.

## Off-chain boundary

The export itself (profiles, agreements, sessions, goals, messages, reviews, credentials and financial references) is built and delivered off-chain.

## Rules

- Only the requesting user can request an export of their own data.
- The manifest lists every data class included, so omissions are visible.
- Repeating a request returns the same receipt instead of creating a new one.

## Events and errors

Event `export_issued(requester_hash, export_hash)`. Errors: `NotAuthorized`, `AlreadyProcessed`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-108 work lives in the other ChainVerse repositories.
