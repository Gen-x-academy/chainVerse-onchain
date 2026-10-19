# MENTOR-060: Generate Secure Meeting Access

**Track:** Scheduling | **Component:** on-chain | **Status:** design

Goal: Generate secure meeting access.

## On-chain

A time-bound access record per session holding a hash of the provider reference and the ledger timestamp at which the access expires.

## Off-chain boundary

The meeting provider console, the lobby experience and the one-time join secrets are served by the app.

## Rules

- Access is minted shortly before an eligible session and expires no later than the session end.
- Cancelling a session revokes any access already minted for it.
- The raw join secret is never stored; only a hash of the provider reference is committed.
- Only a booked participant may mint or read their own access record.
- Access for an in-person session is a check-in code rather than a meeting link, and follows the same expiry rule.

## Events and errors

Events `session_access_minted(session, participant, expires_at)` and `session_access_revoked(session)`. Errors: `NotAuthorized`, `NotBooked`, `SessionNotEligible`, `WindowClosed`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-060 work lives in the other ChainVerse repositories.