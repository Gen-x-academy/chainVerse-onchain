# Mentor Professional Profiles

- Status: Implemented
- Related issue: MENTOR-008 (Identity track)

## Scope

`mentor-profiles` anchors owner-controlled professional profiles to the
ledger. A profile revision commits to an off-chain manifest containing the
headline, biography, expertise, languages, time zone, experience, teaching
approach, and links. The contract does not store those field values.

## Contract state and lifecycle

- A mentor creates one current profile per address. Only that address may
  create, update, change visibility, or deactivate/reactivate it.
- Each manifest or visibility/status change creates a monotonically versioned
  immutable metadata revision and emits an event. Repeating the current
  visibility or status is an idempotent no-op.
- `get_profile` returns only the owner address, commitment, schema version,
  visibility/status, version, and timestamps. Historical metadata revisions
  are queryable by version.
- Profile, revision, and version-counter records use persistent storage with
  a 1–2 year TTL. Reads and writes renew the touched record; clients needing
  continued availability should periodically read the current profile.

## Privacy boundary

The profile document and its links remain off-chain. Store a salted
commitment over the canonical manifest (or encrypted manifest) to reduce
dictionary attacks against low-entropy fields. The commitment is public and
does not hide the underlying data if the input is guessable without a salt.
`Private`, `Matched`, and `Public` are policy metadata only: all ledger state
and transaction arguments are publicly observable, so the off-chain profile
service must enforce visibility and authorization.

## Events

`MPRNEW` announces creation; `MPRUPD` announces a new manifest revision;
`MPRVIS` announces a visibility change; `MPRSTAT` announces activation status.
All include the owner and revision number, and profile-content events include
only the commitment and schema/visibility metadata.

## Validation

Run `cargo test -p mentor-profiles` from `contracts/`.