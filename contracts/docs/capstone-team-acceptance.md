# CAPSTONE-043: Mutual Team Acceptance

> Issue: #1391 · Track: Teams · Status: **specification** — the `capstone-team-acceptance` contract is not yet implemented. Revalidated against `main` at `4471755` (2026-11-20); the audited baseline was `5b62b369`.

## Contract boundary

`capstone-team-acceptance` turns a proposed team into an **active** team. A team becomes active only when every configured member and supervisor has approved the current version of the team terms. Activation does three things in a single atomic transaction:

1. Reserves capacity, both the project's team slots and each supervisor's supervision slots.
2. Runs the conflict-of-interest gate (CAPSTONE-045).
3. Binds roles (CAPSTONE-044) and consumes the members' invitations (CAPSTONE-042).

If any step fails, nothing is written.

The contract stores only addresses, approval flags, the terms commitment, and counters. It holds no funds.

## Data model

```
TeamProposal {
  project_id:       BytesN<32>,
  proposer:         Address,          // MANAGE_TEAM holder
  terms_hash:       BytesN<32>,       // current team terms commitment
  terms_version:    u32,              // bumps on every terms change; resets approvals
  members:          Vec<(Address, BytesN<32> /*invite_id*/)>,  // ≤ 32
  supervisors:      Vec<Address>,     // ≤ 8
  required:         ApprovalPolicy,   // AllMembersAndSupervisors (only policy in v1)
  approvals:        Map<Address, u32>,// address → terms_version approved
  state:            TeamState,
  expires_at:       u64,              // proposal deadline, ≤ 30 days
  revision:         u32,              // CAS counter for every mutation
}

enum TeamState { Draft = 0, Pending = 1, Active = 2, Cancelled = 3, Expired = 4 }

Capacity {
  ProjectSlots(project_id)        -> { max: u32, used: u32 }   // set by project authority
  SupervisorSlots(tenant, Address)-> { max: u32, used: u32 }   // set by tenant authority
}
```

## Lifecycle

```
 Draft ──open──► Pending ──(all approvals at current terms_version)──activate──► Active
   │               │  ▲                                                          
   │               │  └── amend_terms: terms_version++, approvals effectively reset
   │               ├── cancel (proposer / project authority) ──► Cancelled
   └─cancel────────┴── now ≥ expires_at (lazy) ──────────────► Expired
```

- `propose(proposer, project_id, terms_hash, members, supervisors, expires_at)` requires `MANAGE_TEAM` and creates a proposal in the `Draft` state. Each member must have an `Accepted` invitation on CAPSTONE-042 whose `terms_hash` equals the proposal's `terms_hash`. A project can have only one non-terminal proposal at a time (`ProposalExists`).
- `open(proposer, project_id, expected_revision)` moves the proposal from `Draft` to `Pending`. After that, members and supervisors are frozen. To change them, cancel the proposal and propose again.
- `approve(approver, project_id, expected_terms_version)`: the approver must be a listed member or supervisor and must authorize the call. The approval records the `terms_version` it was given for. If the version is not current, the call returns `StaleTerms`, so the approver has to re-read the terms.
- `withdraw_approval(approver, project_id)` works while the proposal is `Pending`.
- `amend_terms(proposer, project_id, expected_revision, new_terms_hash)` increments `terms_version`. Every existing approval now refers to an old version and so no longer counts. Accepting new terms is never implied.
- `activate(caller, project_id, expected_revision)` can be called by anyone. Approvals are the authorization, and activation is permissionless once the conditions hold. In one invocation it:
  1. Checks the state is `Pending`, the proposal has not expired, and `revision == expected_revision`.
  2. Checks that every member and supervisor has approved at `terms_version`.
  3. Calls `capstone-team-conflicts.check_team(project_id, members, supervisors)` and fails with `ConflictBlocked` if a conflict is found.
  4. Checks capacity: `ProjectSlots.used + |members| ≤ max`, and for each supervisor, `used + 1 ≤ max`. If not, it returns `CapacityExceeded`.
  5. Increments every counter.
  6. Calls `capstone-team-invitations.consume(invite_id)` for each member.
  7. Calls `capstone-project-roles.bind(project_id, member, invite.role_id)` for each member.
  8. Sets the state to `Active` and emits `TEAMACT`.

  Soroban transactions are atomic, so if any sub-call returns an error, everything is rolled back, including the capacity counters.
- `release(caller, project_id)`: the project authority calls this when the team completes or is dissolved, to decrement the capacity counters. It runs once per activation (`AlreadyReleased`).

## Authorization and tenant boundary

Authorization is deny-by-default:
- Only listed addresses can approve, and each approves only for themselves.
- Only a `MANAGE_TEAM` holder (CAPSTONE-044) or the project authority can propose, open, amend, or cancel.
- Capacity limits can be set only by the project authority (project slots) or the tenant authority (supervisor slots).

Supervisor capacity is keyed by `tenant`, so two institutions never share or exhaust each other's counters. Every cross-contract call passes the same `project_id`, and the downstream contracts re-check the tenant independently.

"Current-term acceptance" means two things. An approval counts only for the exact `terms_version` it was given for. And every member's invite must have been accepted against the same `terms_hash`. If the terms change after an invite was accepted, `propose` rejects that member (`InviteTermsMismatch`), and the member must be invited again.

## Idempotency, concurrency, and replay

- Each member or supervisor holds one approval entry. Approving again at the same version is a no-op with no event.
- Every mutation compares `expected_revision` with the stored revision. Two concurrent `amend_terms` or `activate` submissions therefore resolve deterministically: the first succeeds, and the second gets `StaleRevision`.
- `activate` on a proposal that is already `Active` returns `InvalidTransition`. Capacity is never reserved twice, and invitation consumption is single-use downstream, which adds a second safeguard.
- Capacity counters change only inside `activate` and `release`, and both run under the single-proposal-per-project rule. No path allows a double reservation.

## On-chain vs off-chain

| On-chain | Off-chain |
|----------|-----------|
| Addresses, invite ids, terms hash and version, approval versions, state, capacity counters, timestamps | Team terms document, supervisor workload rationale, discussion and negotiation, notifications |

## Storage bounds and TTL

- At most 32 members and 8 supervisors per proposal, and at most 40 entries in the approvals map. The `TeamProposal` entry is bounded at roughly 3 KB.
- `activate` makes the largest number of sub-calls: one conflict check, up to 32 `consume` calls, and up to 32 `bind` calls. Its resource budget must be measured at the bounds and pinned in a test. If it exceeds the Soroban limit, reduce `MAX_TEAM_SIZE` across CAPSTONE-042, 043, and 044 together.
- The standard capstone TTL applies. Active proposals must be renewed by a keeper for the project's duration.

## Events

| Topic | Data |
|-------|------|
| `TEAMPROP` | `(project_id, proposer, terms_version)` |
| `TEAMOPEN` | `(project_id, revision)` |
| `TEAMAPPR` | `(project_id, approver, terms_version)` |
| `TEAMWDRW` | `(project_id, approver)` |
| `TEAMTERM` | `(project_id, terms_hash, terms_version)` |
| `TEAMACT` | `(project_id, member_count, supervisor_count)` |
| `TEAMCAN` | `(project_id, cancelled_by)` |
| `TEAMEXP` | `(project_id)` |
| `TEAMREL` | `(project_id)` |
| `CAPSET` | `(scope_key, max)` |

## Errors (stable codes)

| Code | Name | Caller action |
|------|------|---------------|
| 1 | `NotInitialized` | Initialize the contract |
| 2 | `Unauthorized` | Sign with the correct account |
| 3 | `MissingPermission` | Needs `MANAGE_TEAM` |
| 4 | `ProjectNotFound` | Register the project |
| 5 | `ProposalExists` | Cancel or finish the current proposal |
| 6 | `ProposalNotFound` | Propose first |
| 7 | `InvalidTransition` | Re-read the state |
| 8 | `StaleRevision` | Re-read and retry with the current revision |
| 9 | `StaleTerms` | Review the current terms version and approve again |
| 10 | `NotParticipant` | Only listed members and supervisors can approve |
| 11 | `ApprovalsIncomplete` | Wait for the remaining approvals |
| 12 | `InviteNotAccepted` | The member must accept their invitation |
| 13 | `InviteTermsMismatch` | Re-invite the member under the current terms |
| 14 | `ConflictBlocked` | Resolve the conflict of interest (CAPSTONE-045) |
| 15 | `CapacityExceeded` | Free capacity or choose another supervisor |
| 16 | `ProposalExpired` | Propose again |
| 17 | `TeamTooLarge` | Reduce the number of members or supervisors |
| 18 | `AlreadyReleased` | — |

## Required tests

- **Authorization tree:** approval by a non-participant, approval on someone else's behalf, propose or amend without `MANAGE_TEAM`, and capacity changes by a non-authority or the wrong tenant.
- **Mutual acceptance:** activation fails until the last approval arrives. Amending the terms invalidates all approvals, and approving again at the new version unlocks activation.
- **Atomicity:** if the conflict gate, capacity check, `consume`, or `bind` fails, every counter, invite, and roster entry must be unchanged.
- **Concurrency and replay:** two `activate` calls with the same revision succeed once. A replayed approval at an old `terms_version` is rejected. A double `release` fails.
- **Capacity:** test at exactly `max`, at `max + 1`, with a supervisor shared across two projects, and with tenant isolation of supervisor counters.
- **Expiry:** test at the boundary of `expires_at`, and check that `TEAMEXP` is emitted once.
- **Bounds and budget:** a 32-member, 8-supervisor `activate` must stay within a recorded ceiling.
- **Events and errors:** assert the exact event tuples and pin the discriminants.
- **Upgrade:** decode a fixture `TeamProposal` and `Capacity` after a WASM upgrade.

## Dependencies

`capstone-identifiers`, CAPSTONE-042 `capstone-team-invitations` (accepted invites, `consume`), CAPSTONE-044 `capstone-project-roles` (`MANAGE_TEAM` check, `bind`), CAPSTONE-045 `capstone-team-conflicts` (`check_team`), and optionally `capstone-call-terms` (as the source of `terms_hash`).

## Rollout and rollback

**Rollout order:**
1. Deploy CAPSTONE-044, CAPSTONE-042, and CAPSTONE-045, then this contract.
2. Allowlist this contract in CAPSTONE-044 (`bind`), CAPSTONE-042 (`consume`), and CAPSTONE-045 (`record_check`).
3. Have the tenant authorities set supervisor capacity.
4. Enable proposals.

**Rollback:** an admin flag pauses `propose`, `open`, and `activate`. Approvals already given are kept, and active teams are unaffected. A conflict-gate bypass flag exists only for incident response. It emits an event and must be time-boxed. Do not reset capacity counters by hand. Use `release` for each project.

## Corresponding CAPSTONE-043 work in other repositories

- **Backend:** the team proposal API, terms versioning and diffs, approval reminders keyed on `(project_id, terms_version, approver)`, a capacity admin, and an indexer projection.
- **Frontend:** the team approval dashboard (who approved which version), the "terms changed, please re-approve" flow, and capacity indicators for supervisors.
