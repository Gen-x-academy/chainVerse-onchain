# MENTOR-110: Mentorship Secrets and Sensitive Fields

**Track:** Privacy | **Component:** on-chain | **Status:** design

Goal: Protect mentorship secrets and sensitive fields.

## On-chain

Only a ciphertext hash, a field class and a key version per record.

## Off-chain boundary

Provider tokens, identity references, private notes, precise locations, evidence keys and payout details are encrypted off-chain.

## Rules

- Fields are classified before storage; unclassified fields are rejected.
- Key rotation raises the key version and never exposes plaintext.
- Only the record owner or an authorized rotator may change a record.

## Events and errors

Event `field_rotated(record, key_version)`. Errors: `UnknownFieldClass`, `NotAuthorized`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-110 work lives in the other ChainVerse repositories.
