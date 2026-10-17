# MENTOR-100: Portable Mentorship Achievements

**Track:** Reputation | **Component:** on-chain | **Status:** design

Goal: Export portable mentorship achievements.

## On-chain

The credential hash, issuer address and revocation status, which let anyone check an exported file.

## Off-chain boundary

The exported credential file (standards-friendly format) is built and shared by the recipient.

## Rules

- Only the recipient can request an export.
- The export carries an integrity proof and issuer metadata.
- Sharing is controlled by the recipient; the contract never publishes the file.

## Events and errors

Event `export_requested(credential)`. Errors: `NotAuthorized`, `Revoked`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-100 work lives in the other ChainVerse repositories.
