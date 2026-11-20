# CAPSTONE-042: Team Invitations

> Issue: #1390 · Track: Teams · Status: **specification** — the `capstone-team-invitations` contract is not yet implemented. Revalidated against `main` at `4471755` (2026-11-20); the audited baseline was `5b62b369`.

## Contract boundary

`capstone-team-invitations` records scoped invitations to join a capstone project team. An invitation names the invitee address and the intended role (from CAPSTONE-044). It carries a commitment to the team terms, an expiry, and an acceptance state. It is a **commitment and state primitive only**: it does not deliver notifications, create memberships, or reserve capacity. Accepting an invitation records the invitee's consent. Membership itself only comes into being at team activation (CAPSTONE-043).

## Data model

```
Invitation {
  invite_id:        BytesN<32>,  // sha256(project_id ‖ invitee ‖ role_id ‖ client_nonce)
  project_id:       BytesN<32>,  // capstone-identifiers public id
  inviter:          Address,     // must hold MANAGE_TEAM
  invitee:          Address,
  role_id:          u32,         // CAPSTONE-044 role, pinned at role_version
  role_version:     u32,
  terms_hash:       BytesN<32>,  // commitment to the team terms / call-terms version
  context_hash:     BytesN<32>,  // commitment to the privacy-safe project summary shown to the invitee
  expires_at:       u64,         // ledger timestamp
  state:            InviteState,
  created_at:       u64,
  decided_at:       u64,         // 0 until decided
}

enum InviteState { Pending = 0, Accepted = 1, Declined = 2, Revoked = 3, Expired = 4 }
```

`invite_id` is derived deterministically from the inputs and a client-supplied 32-byte `client_nonce`. Because of this, a retry of the same invitation request hits the same key instead of creating a duplicate.

## Lifecycle

```
            accept (invitee, before expiry)
 Pending ───────────────────────────────► Accepted ──► consumed by CAPSTONE-043 activation
   │  └── decline (invitee) ─────────────► Declined
   │  └── revoke (MANAGE_TEAM) ──────────► Revoked
   └───── now ≥ expires_at (lazy) ───────► Expired
```

- `invite(inviter, project_id, invitee, role_id, terms_hash, context_hash, expires_at, client_nonce)` requires the following, in this order:
  1. `inviter.require_auth()`.
  2. `has_permission(project_id, inviter, MANAGE_TEAM)` on CAPSTONE-044.
  3. The role exists and is not retired.
  4. `invitee ≠ inviter`.
  5. `now < expires_at ≤ now + MAX_INVITE_TTL_SECS (30 days)`.
  6. The invitee has no other `Pending` or `Accepted` invite on this project (`DuplicateInvite`).
  7. The project has fewer than `MAX_OPEN_INVITES = 32` pending invites.
- `accept(invitee, invite_id, expected_terms_hash)` requires the invitee's auth. The invite must be `Pending` and unexpired, and `expected_terms_hash == terms_hash`. If the terms hash has changed, the call returns `TermsMismatch`, because consent applies only to the terms the invitee was shown. If the role has been revised since the invite was issued (`role_version` is stale), it returns `StaleRole`, and the inviter must reissue the invite.
- `decline(invitee, invite_id)` and `revoke(caller, invite_id)` both move the invite to a terminal state. An accepted invite can still be revoked up to the point of activation. After activation, use role unbinding in CAPSTONE-044.
- Expiry is evaluated lazily. Every read or write after `expires_at` treats a pending invite as `Expired`, persists that state, and emits `INVEXP` once.
- `consume(acceptance_contract, invite_id)` is called only by CAPSTONE-043. It marks an `Accepted` invite as used by an activation, so the same consent can never seed two activations (`AlreadyConsumed`).

Every terminal state is final. A transition from any terminal state returns `InvalidTransition`.

## Authorization and tenant boundary

Authorization is deny-by-default:
- Only the invitee can accept or decline.
- Only a `MANAGE_TEAM` holder on the **same** `project_id`, or the original inviter, can revoke.
- Only the allowlisted acceptance contract can call `consume`.

The tenant boundary comes from `project_id` → `capstone-identifiers` authority. No call accepts a `project_id` that differs from the one stored on the invite.

## Idempotency and replay

- Re-submitting an identical `invite` (same derived `invite_id`, same fields) returns the existing id with no event. A colliding id with different fields returns `InviteIdConflict`.
- Re-submitting `accept`, `decline`, or `revoke` on an invite that is already in that exact state returns `Ok` without emitting an event. Any other state returns `InvalidTransition`.
- `consume` is single-use.
- The Soroban auth nonce prevents signature replay. `expected_terms_hash` prevents an old signed acceptance from applying to new terms.

## On-chain vs off-chain

| On-chain | Off-chain |
|----------|-----------|
| ids, addresses, role id/version, terms and context hashes, expiry, state, timestamps | Invitation message, project title/abstract, invitee email/name, team terms document, notification delivery |

`context_hash` commits to the project summary the invitee actually saw. That summary must exclude proposal content and any personal data about other members. Notifications are sent off-chain, triggered by the indexer seeing `INVSENT`. Delivery is de-duplicated per `invite_id`, so a retried transaction never produces a second notification.

## Storage bounds and TTL

- `Invite(invite_id)` is fixed-size.
- `OpenInvites(project_id)` holds at most 32 ids, and `ActiveInvite(project_id, invitee)` is a pointer used for duplicate detection.
- Entries in a terminal state are kept for audit at the standard capstone TTL (`3_110_400`/`6_220_800` ledgers). They are not extended after reaching a terminal state, so they eventually archive.

## Events

| Topic | Data |
|-------|------|
| `INVSENT` | `(invite_id, project_id, invitee, role_id, expires_at)` |
| `INVACC` | `(invite_id, invitee, terms_hash)` |
| `INVDEC` | `(invite_id, invitee)` |
| `INVREV` | `(invite_id, revoked_by)` |
| `INVEXP` | `(invite_id)` |
| `INVUSE` | `(invite_id, project_id)` |

## Errors (stable codes)

| Code | Name | Caller action |
|------|------|---------------|
| 1 | `NotInitialized` | Deploy and initialize |
| 2 | `Unauthorized` | Sign with the correct account |
| 3 | `MissingPermission` | The inviter needs `MANAGE_TEAM` |
| 4 | `RoleNotFound` | Choose a valid role |
| 5 | `RoleRetired` | Choose an active role |
| 6 | `SelfInvite` | The invitee must differ from the inviter |
| 7 | `InvalidExpiry` | Expiry must be in the future and ≤ 30 days |
| 8 | `DuplicateInvite` | Revoke or wait out the existing invite |
| 9 | `TooManyOpenInvites` | Revoke stale invites |
| 10 | `InviteNotFound` | Check `invite_id` |
| 11 | `InviteExpired` | Request a new invite |
| 12 | `InvalidTransition` | Re-read the state; the invite is already terminal |
| 13 | `TermsMismatch` | Re-fetch the terms, review them, and accept again |
| 14 | `StaleRole` | The inviter must reissue against the current role version |
| 15 | `AlreadyConsumed` | The invite was already used for activation |
| 16 | `InviteIdConflict` | Use a fresh `client_nonce` |

## Required tests

- **Authorization tree:** accept or decline by a non-invitee, revoke by a non-manager or a manager of another project, and `consume` by a non-allowlisted contract.
- **Replay:** an identical `invite` returns the same id with one event total, a double `accept` succeeds without a second event, a second `consume` fails, and an old signed `accept` against new terms returns `TermsMismatch`.
- **Expiry:** use ledger-time manipulation at `expires_at - 1`, `expires_at`, and `expires_at + 1`, and check that `INVEXP` is emitted exactly once.
- **State transitions:** cover the full matrix of every `InviteState` × every action.
- **Bounds:** the 33rd open invite fails. Record the budget for `invite` and `accept`.
- **Events and errors:** assert the exact event tuples and pin the error discriminants.
- **Upgrade:** decode a fixture `Invitation` after a WASM upgrade and check `storage_version() == 1`.

## Dependencies

`capstone-identifiers` (project and tenant), CAPSTONE-044 `capstone-project-roles` (permission and role version), `capstone-call-terms` (the `terms_hash` source when an invite references a call-terms version), and CAPSTONE-043 (the only caller allowed to `consume`).

## Rollout and rollback

**Rollout:** deploy after CAPSTONE-044. Set the CAPSTONE-043 allowlist once that contract exists. Until then, invites can be accepted but not consumed.

**Rollback:** disable `invite` off-chain. Existing invites expire naturally within 30 days. No funds are held, so rollback is not destructive.

## Corresponding CAPSTONE-042 work in other repositories

- **Backend:** the invitation API, canonical generation and hashing of the terms and context summary, a notification worker keyed on `invite_id`, and an indexer projection.
- **Frontend:** the invite composer (role, expiry), the invitee inbox with a terms diff and hash verification, and the accept and decline flows.
