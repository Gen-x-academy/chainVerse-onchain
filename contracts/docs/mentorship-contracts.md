# mentorship-contracts

Contract: `mentorship-contracts`  
Version: 2

## Purpose

Publish bilateral, immutable, versioned mentorship terms and expose typed
queries over their history. Both mentor and mentee authorize every version.
The first publication fixes the parties for that agreement ID; amendments
must be co-authorized by those same parties.

## ABI schemas

- `PublishMentorshipCommand` carries agreement ID, schema version, parties,
  fee, session count, date range, terms digest, and optional evidence hashes.
- `GetMentorshipContractQuery` requests a specific version or the latest one.
- `ListMentorshipContractsQuery`, `PageRequest`, and
  `ContractVersionPage` provide exclusive-cursor pagination, capped at 50
  records per page. Version history is capped at 100 records per agreement.
- `Money` uses a Soroban asset address and a positive amount in that asset's
  smallest units.
- `DateRange` uses UTC ledger timestamps in seconds and requires
  `starts_at < ends_at`.
- `EvidenceReference` stores only content and reference digests. Evidence,
  locators, and agreement text remain off-chain; `terms_hash` commits to the
  full agreed terms without publishing them.
- `MentorshipContractVersion` includes the schema version and publication
  status. Future schema additions should use a new schema version rather than
  reinterpreting previously published records.

## Commands and queries

`publish(command)` requires authorization from both named parties. It validates
schema version, positive fee, nonzero session count, ordered dates, and
unchanged parties, then appends a new immutable version. Queries are
`get_contract_version`, `get_latest_version`, and `list_contract_versions`.

## Event

`MENTPUB` emits `MentorshipContractPublished` with agreement ID, version,
schema version, parties, terms hash, status, and publication timestamp. The
event deliberately omits evidence reference hashes.

## Errors

`ContractError` distinguishes missing agreements and versions, unsupported
schemas, invalid terms or pagination, invalid cursors, version overflow or
limits, and attempts to change the parties.

## Program capacity and waitlists

`create_program` records an organizer, seat capacity, offer expiry duration,
and automatic-promotion policy. `request_seat` immediately reserves an
available seat as an expiring offer; when no seat is available, it appends the
participant to the bounded FIFO waitlist. `confirm_seat` converts a live offer
to a confirmed seat. Participants may decline offers or release confirmed
seats; anyone may call `expire_offer` once the ledger timestamp reaches the
offer expiry. Released capacity promotes the FIFO head automatically when
configured, otherwise only the organizer can call `promote_next`.

The invariant `reserved_seats + confirmed_seats <= capacity` is checked on
updates. Capacity cannot be reduced below committed seats. Increasing capacity
with automatic promotion enabled offers seats to queued participants in order.
`get_waitlist_page` and `get_seat_record` expose ordered addresses and public
seat state; no applicant profile or private application content is stored.

Events include `CAPSET`, `WAITADD`, `SEATOFR`, `SEATCNF`, `SEATREL`, and
`SEATEXP`. Offer expiry is ledger-driven but requires a transaction to settle;
the public `expire_offer` call is that permissionless settlement hook.

## Privacy and storage

Persistent storage contains only public party addresses, the payment asset and
amount, schedule timestamps, status, schema/version counters, and hashes.
Mentorship descriptions, notes, contact information, and evidence contents
must remain off-chain.