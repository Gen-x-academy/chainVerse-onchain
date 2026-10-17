# MENTOR-098: Public Credential Verification

**Track:** Reputation | **Component:** on-chain | **Status:** design

Goal: Verify mentorship credentials publicly.

## On-chain

A credential hash, issuer address and status, readable by anyone.

## Off-chain boundary

Recipient profile data is never exposed.

## Rules

- A caller supplies a credential reference or proof and receives only valid, revoked, superseded or unknown.
- The result contains no other profile data.
- Verification is a read-only call.

## Events and errors

No events. A missing credential returns `unknown`, not an error.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-098 work lives in the other ChainVerse repositories.
