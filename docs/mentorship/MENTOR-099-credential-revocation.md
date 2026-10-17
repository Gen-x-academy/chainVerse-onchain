# MENTOR-099: Revoking and Superseding Credentials

**Track:** Reputation | **Component:** on-chain | **Status:** design

Goal: Revoke or supersede credentials.

## On-chain

A status per credential (valid, revoked, superseded), a reason code and a pointer to the replacing credential.

## Off-chain boundary

Explanations for the reason stay off-chain.

## Rules

- Only the issuer can revoke or supersede.
- Revoking needs a reason from a fixed list.
- History stays readable: the old record is never deleted.

## Events and errors

Events `credential_revoked` and `credential_superseded`. Errors: `NotAuthorized`, `UnknownReason`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-099 work lives in the other ChainVerse repositories.
