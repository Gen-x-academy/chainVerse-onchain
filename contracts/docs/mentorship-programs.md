# Mentorship Programs Contract

`contracts/mentorship-programs` creates and manages organization-owned
mentorship program drafts. Each program uses a stable `BytesN<32>` ID and is
owned by the organization `Address` supplied at creation. That organization
address must authorize creation, draft edits, and every status transition.
The contract does not maintain a separate organization-member registry;
organization account or contract authorization is the membership boundary.

## Stored Data And Privacy

The record stores the audience and outcomes as 32-byte commitments, not the
underlying audience descriptions or outcome content. Keep that private content
off-chain. Commitments should be computed from canonical content with a
high-entropy secret salt, since unsalted hashes of predictable text can be
guessed. Other stored fields are capacity, duration in days, delivery mode,
start/end ledger timestamps, visibility, status, owner, and timestamps.

Visibility is a program setting, not a confidentiality guarantee: Soroban
contract storage and events are public. Do not put private data or reversible
references in fields or events. The contract emits `MENTNEW` on creation,
`MENTEDIT` on draft edits, and `MENTSTAT` on lifecycle changes. Consumers can
reconstruct status history from events; only the current status and latest
update timestamp are kept in the record.

## Rules

- Creation always starts in `Draft`; IDs are unique and immutable.
- Capacity and duration must be greater than zero; `starts_at` must precede
  `ends_at`.
- Only the owning organization can edit, and only while the program is a draft.
- Legal transitions are `Draft→Published`, `Draft→Archived`,
  `Published→Paused/Closed`, `Paused→Published/Closed`, and
  `Closed→Archived`. Archived is terminal.
- Persistent records receive a roughly one-year TTL extension on writes/reads.

The contract exposes its Soroban ABI through the exported contract methods,
types, and errors. No separate manually maintained ABI file is needed.