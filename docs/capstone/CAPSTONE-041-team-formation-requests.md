# CAPSTONE-041: Project Team Formation Requests

**Track:** Teams | **Component:** on-chain | **Status:** specified (not yet implemented)
**Issue:** #1389 | **Audit baseline:** `main` @ `5b62b369` (2026-11-18), revalidated against `main` @ `4471755`

Goal: let eligible participants propose teams with requested roles, skill
coverage, project linkage, expiry and initiator.

## Revalidation against main

There is no team primitive on `main`. Related contracts:
`capstone-identifiers` (project linkage via `public_id`),
`capstone-opportunity-details` (seat counts / open state) and
`capstone-profile-visibility` (which profile fields others may see; consulted
off-chain when matching).

## On-chain scope

A new contract, `contracts/capstone-teams`:

```text
create_team_request(initiator, tenant_id, team_id, project_id,
                    roles: Vec<RoleSlot> /* <= 8 */, skills_commitment: BytesN<32>,
                    expires_at: u64, intent_id) -> Result<(), TeamError>
respond(member, tenant_id, team_id, slot: u32, accept: bool, intent_id)
cancel_request(initiator, tenant_id, team_id)
finalize(initiator, tenant_id, team_id)        // Open -> Formed
get_team(tenant_id, team_id) -> Option<TeamRequest>
```

`TeamRequest`:

| Field | Type | Notes |
| --- | --- | --- |
| `team_id` | `BytesN<32>` | Opaque, client-generated |
| `tenant_id` | `BytesN<32>` | |
| `project_id` | `BytesN<32>` | Must exist in `capstone-identifiers` |
| `initiator` | `Address` | Counts as the first member |
| `roles` | `Vec<RoleSlot>` | `{ role_code: u32, filled_by: Option<Address> }`, max 8 |
| `skills_commitment` | `BytesN<32>` | Hash of the requested skill-coverage matrix |
| `status` | `TeamStatus` | `Open`, `Formed`, `Cancelled`, `Expired` |
| `created_at` / `expires_at` | `u64` | `expires_at - created_at <= MAX_TTL` (e.g. 30 days) |

## Rules

- **Eligibility, deny by default.** The initiator and every responding member
  must hold the participant role in `tenant_id`. `project_id` must belong to
  the same tenant.
- **One open request per initiator per project.** A second request gives
  `DuplicateOpenRequest`.
- **One membership per participant per project.** Accepting a slot while
  already a member of another `Open`/`Formed` team for the same project gives
  `AlreadyOnTeam`. The membership index `Member(tenant, project, address)` is
  written in the same invocation as the slot fill, so concurrent accepts
  cannot double-book.
- **Slots** fill first-come. Accepting a filled slot gives `SlotTaken`.
  `accept = false` records nothing on-chain (declines are off-chain), so it
  creates no side effect.
- **Expiry** is evaluated lazily. Any mutation at `now > expires_at`
  transitions to `Expired`, releases membership indexes, and returns
  `RequestExpired`. Reads report `Expired` without writing.
- **Finalize** requires every slot filled and `now <= expires_at`. A `Formed`
  team is immutable here. Later membership changes belong to a separate work
  item.
- **Idempotency.** `intent_id` replays with the same payload are no-ops. With
  a different payload they give `IntentConflict`.

## Lifecycle

```text
(none) --create--> Open --finalize (all slots filled)--> Formed
                    |--cancel (initiator)-------------> Cancelled
                    \--any call after expires_at------> Expired
```

Every other transition gives `InvalidTransition`.

## Errors (stable codes)

| Code | Name |
| --- | --- |
| 1 | `Unauthorized` |
| 2 | `TenantMismatch` |
| 3 | `ProjectNotFound` |
| 4 | `TooManyRoles` / empty roles |
| 5 | `InvalidExpiry` (past, or beyond `MAX_TTL`) |
| 6 | `DuplicateOpenRequest` |
| 7 | `TeamNotFound` |
| 8 | `SlotNotFound` |
| 9 | `SlotTaken` |
| 10 | `AlreadyOnTeam` |
| 11 | `RequestExpired` |
| 12 | `IncompleteTeam` |
| 13 | `InvalidTransition` |
| 14 | `IntentConflict` |

## Events

- `("CAPTEAM", "new")` → `(tenant_id, team_id, project_id, role_count, expires_at)`
- `("CAPTEAM", "join")` → `(tenant_id, team_id, slot)`. The member address is
  omitted from the event and readable through `get_team` only.
- `("CAPTEAM", "state")` → `(tenant_id, team_id, status)`

## Off-chain boundary

- Skill descriptions, coverage matrix, role descriptions, invitation messages,
  declines and participant profiles stay in the teams service. The chain holds
  `skills_commitment` and numeric `role_code`s only.
- Discovery and matching of candidates (respecting `capstone-profile-visibility`)
  are off-chain.
- Notifications for invites and expiry are sent from the off-chain outbox,
  driven by events and deduplicated by `(team_id, seq)`.

## Test plan

- Auth: initiator vs member vs outsider, and a foreign-tenant project.
- Replay: the same `intent_id` on create and respond is idempotent.
- Concurrency: two members accept the same slot (one gets `SlotTaken`); one
  member accepts slots on two teams for the same project (`AlreadyOnTeam`).
- Expiry: a call at `expires_at` passes and `+1` expires, releasing membership indexes.
- Bounds: 8 roles pass and 9 fail; `MAX_TTL` is enforced.
- Transitions: full matrix. Finalize with an empty slot gives `IncompleteTeam`.
- Event schema snapshots (no member address), error codes, budget, upgrade decode.

## Rollout / rollback

Deploy `capstone-teams` independently. CAPSTONE-038 reads `Formed` status via
cross-contract call, so deploy teams first. Rollback: stop creating requests
off-chain, and let open requests expire naturally (bounded by `MAX_TTL`).

## Cross-repository work (CAPSTONE-041)

- **Backend:** team request service, skill matrix hashing, invite outbox, expiry jobs.
- **Frontend:** create-team form (roles, skills, expiry), invite/accept flow, team status card.
