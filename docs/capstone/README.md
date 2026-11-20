# Capstone & Research Projects — On-chain Design Docs

| ID | Doc | Track | Contract |
| --- | --- | --- | --- |
| CAPSTONE-038 | [Submit proposals atomically](CAPSTONE-038-submit-proposals-atomically.md) | Proposals | `capstone-proposals` (new) |
| CAPSTONE-039 | [Withdrawal and resubmission](CAPSTONE-039-proposal-withdrawal-resubmission.md) | Proposals | `capstone-proposals` |
| CAPSTONE-040 | [History and status timeline](CAPSTONE-040-proposal-history-timeline.md) | Proposals | `capstone-proposals` |
| CAPSTONE-041 | [Team formation requests](CAPSTONE-041-team-formation-requests.md) | Teams | `capstone-teams` (new) |

## Shared principles

- **Privacy:** only hashes, opaque ids, numeric codes, states and timestamps go
  on-chain. Proposal content, reviewer comments, skills and profiles stay off-chain.
- **Authorization:** `require_auth` on every mutation, role checks deny by
  default, and every key is scoped by `tenant_id`.
- **Idempotency:** every mutation takes an `intent_id`. Replays are no-ops and
  conflicting replays fail.
- **Errors:** `#[contracterror]` codes are stable and append-only. They are
  never renumbered across upgrades.

## Deployment order

`capstone-identifiers` → `capstone-call-terms` / `capstone-delivery-modes` →
`capstone-teams` (041) → `capstone-proposals` (038 → 039 → 040).
