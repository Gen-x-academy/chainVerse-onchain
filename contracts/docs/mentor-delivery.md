# Mentorship Delivery Configuration

- Status: Implemented
- Related issue: MENTOR-020 (Programs track)

## Scope

`mentor-delivery` lets a program owner configure `Remote`, `InPerson`, or
`Hybrid` delivery and publish versioned safety/location policy metadata.
Only the owner address supplied at creation can update that program's config.

## Mode rules

- `Remote` requires `NotApplicable` location visibility, no location
  commitment, and no physical-safety attestation.
- `InPerson` and `Hybrid` require a location commitment, a non-`NotApplicable`
  visibility setting, a nonzero safety-policy hash, and the owner's explicit
  physical-safety confirmation.
- `Hidden`, `MatchedOnly`, and `PublicArea` describe what the off-chain
  service may reveal. `PublicArea` is for coarse area information only; a
  precise venue must not be disclosed publicly.
- Every configuration requires a nonzero safety-policy commitment and schema
  version. Policy contents and exact locations remain off-chain.

## Privacy boundary

The contract stores program and owner addresses/IDs, mode and visibility
enums, status-like safety metadata, schema/version numbers, timestamps, and
hashes. Use salted commitments for location/policy data to avoid exposing
guessable values. All ledger state and arguments remain public. Visibility
settings are not access control; the off-chain service must enforce them and
manage participant authorization.

## Versioning and events

Creation starts at version 1. Each successful update appends an immutable
revision and increments the version. `MDNEW` and `MDUPD` include the program,
owner, version, mode, visibility, location/policy hashes, physical-safety
confirmation, and schema version. Profile revisions use persistent storage
with a 1–2 year TTL; reads renew the touched record and version counter.

## Validation

Run `cargo test -p mentor-delivery` from `contracts/`.