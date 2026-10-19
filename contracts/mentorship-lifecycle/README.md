# Mentorship Lifecycle

This contract provides the Soroban state and authorization primitive for a
mentorship engagement. It supports `Draft`, `Published`, `Accepting`,
`Matched`, `Active`, `Paused`, `Completed`, `Cancelled`, `Disputed`, and
`Archived` with a closed transition graph.

Allowed transitions are:

- `Draft` → `Published` or `Cancelled`
- `Published` → `Accepting` or `Cancelled`
- `Accepting` → `Matched` (through `match_mentorship`) or `Cancelled`
- `Matched` → `Active` or `Cancelled`
- `Active` → `Paused`, `Completed`, `Disputed`, or `Cancelled`
- `Paused` → `Active`, `Disputed`, or `Cancelled`
- `Completed` → `Disputed` or `Archived`
- `Disputed` → `Active`, `Completed`, or `Cancelled`
- `Cancelled` → `Archived`
- `Archived` is terminal

Mentor and mentee addresses are public authorization principals. The record
contains an opaque 32-byte ID and an immutable 32-byte commitment hash; it does
not store conversation content, personal details, meeting schedules, dispute
evidence, or review notes. Those remain off-chain and can be integrity-checked
against the commitment by the authorized application. Do not derive IDs or
commitments directly from guessable private data without salting them.

Lifecycle events contain the ID, prior and next statuses, actor, and ledger
time. They intentionally omit the commitment and any private content. Events
are transactionally emitted with successful state changes, but indexer
availability and retention remain an off-chain operational responsibility.

Run focused tests from this directory's parent workspace with:

```sh
cargo test -p mentorship-lifecycle
```