# MENTOR-111: Mentorship Audit Log

**Track:** Privacy | **Component:** on-chain | **Status:** design

Goal: Add mentorship audit log.

## On-chain

A hash chain: each entry commits to the previous entry's hash, actor, tenant, target, action, outcome, correlation ID and ledger time.

## Off-chain boundary

Full entry details and any personal data stay in the off-chain log; only the chain head and entry hashes are published.

## Rules

- Entries are append-only; nothing can be edited or removed.
- Only an authorized logger address may append, and a repeated correlation ID is ignored.
- Anyone can verify the chain by recomputing hashes.

## Events and errors

Event `audit_appended(tenant, seq, entry_hash)`. Errors: `NotAuthorized`, `DuplicateCorrelation`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-111 work lives in the other ChainVerse repositories.
