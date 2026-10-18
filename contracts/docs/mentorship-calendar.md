# Mentorship Calendar Contract

- Status: Implemented
- Owner: Mentorship On-chain working group
- Related issue: #1250 — `MENTOR-054` Connect external calendar availability
  (Scheduling track)
- Depends on / relates to: `contracts/mentorship-availability` (busy windows
  are consumed as `Unavailable` exceptions) and, off-chain, the provider OAuth
  apps, the secret vault that holds encrypted tokens, and the sync worker

## Scope

`contracts/mentorship-calendar` lets a participant connect an external
calendar and import free/busy time without copying event content. It
implements the **On-chain** half of `MENTOR-054`:

- **Connections.** `connect` stores the provider, a scope bitmask, a
  `token_commitment` (hash over the encrypted token), a `token_ref` handle to
  the ciphertext in the off-chain vault, and the token expiry. The token
  itself is never written on-chain.
- **Scoped access.** The scope mask must include `SCOPE_FREE_BUSY` and may
  only contain `SCOPE_FREE_BUSY` and `SCOPE_TIMEZONE`. Any content scope
  (titles, attendees, descriptions) or unknown bit is rejected with
  `ScopeNotAllowed`, and a mask without free/busy is `InvalidScope`.
- **Replay-safe sync.** `record_busy(owner, id, epoch, payload_hash, windows)`
  accepts strictly increasing epochs. An identical retry (same epoch, same
  payload hash) is a no-op; an equal or older epoch with a different payload
  is `StaleSyncEpoch`. An accepted snapshot replaces the previous windows
  atomically, so a retry can never duplicate busy time. Windows are
  normalized (sorted and merged) deterministically, so overlapping provider
  data produces the same stored list every time.
- **Graceful degradation.** `mark_sync_failed` sets a degraded flag and an
  error code while **keeping** the last known busy windows, so a provider
  outage narrows nothing silently; a successful sync clears the flag.
  `connection_healthy` reports active, unexpired, non-degraded connections.
- **Expiry and disconnect remove future access.** A sync after the token
  expiry fails with `ConnectionExpired`, and `expire_connection` (once the
  expiry has passed) or `disconnect` deletes the connection's busy windows
  immediately. `busy_windows` returns nothing for revoked or expired
  connections.

**Not implemented here:** OAuth, token encryption/decryption, provider API
calls, and mapping busy windows into availability. The off-chain worker owns
the provider integration; this contract only records the commitments, the
sync epochs, and the busy intervals.

## Privacy

- Only busy intervals are retained. The stored `BusyWindow` is a pair of
  timestamps; there is no field for a title, attendee, description,
  location, or event id, so the contract cannot retain content even if a
  caller tried.
- Tokens are represented by a commitment plus a vault reference. A leaked
  ledger or a public `get_connection` shows neither the token nor anything
  decryptable from it.
- Connection ids, provider names, scopes, and sync epochs are public. The
  mapping from a participant address to a real identity is an off-chain
  concern.
- Because revoked and expired connections delete their busy windows, a
  disconnect also removes the historical busy pattern from this contract
  (off-chain vault retention is governed by the privacy policy).

## Ownership

Only the connection owner can sync, report failures, expire, or disconnect,
and every entry point requires that owner's signature. Busy windows are keyed
by `(owner, connection_id)`, so one participant cannot read or write another
participant's connection even with a guessed id. There is no admin override.

## Migration

New contract; no prior on-chain state. After deployment, the off-chain worker
must be able to store encrypted tokens in the vault and write only their
commitment and reference here. `CONTRACT_VERSION` is exposed as `version()`
for upgrade validation; status and sync bookkeeping are additive fields on
`CalendarConnection`.

## Operational impact

- **Storage/TTL.** Per owner: up to 8 connections; per connection: up to 128
  normalized busy windows. All records are `persistent` with a ~1-year TTL
  refreshed on read and write. A sync accepts at most 128 input windows, each
  at most 7 days long and no further than 60 days ahead, so both storage and
  validation are bounded.
- **Queries.** `busy_windows` pages at most 64 windows over a span of at most
  90 days.
- **Events.** `CALCONN`, `CALSYNC`, `CALFAIL`, `CALEXP`, and `CALDISC` carry
  the owner, connection id, provider/epoch, or error code for the sync worker
  and indexers.
- **Failure modes.** A provider outage leaves stale-but-present busy windows
  and a degraded flag; the worker should retry with the next epoch. A token
  expiry stops syncs immediately and is surfaced as an actionable error, not a
  silent empty calendar. Disconnect is irreversible: reconnecting uses a new
  connection id with a fresh epoch.
- **Rollout.** Ship behind the mentorship feature flag and start with
  read-only free/busy scopes. Only enable a provider after its OAuth
  verification is approved and its token-encryption path has been reviewed;
  monitor `CALFAIL` rates rather than assuming sync success.
- **Cross-repository work.** The matching `MENTOR-054` work in the other
  ChainVerse repositories owns the provider integrations, the encrypted token
  vault, and the sync scheduler that produces epochs and payload hashes.
