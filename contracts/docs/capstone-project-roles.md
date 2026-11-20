# CAPSTONE-044: Project Roles and Responsibilities

> Issue: #1392 · Track: Teams · Status: **specification** — the `capstone-project-roles` contract is not yet implemented. Revalidated against `main` at `4471755` (2026-11-20); the audited baseline was `5b62b369`.

## Contract boundary

`capstone-project-roles` is the role registry for a capstone project team. For each project it stores the set of roles a team may use and binds each team member to exactly one role. Every role has a bounded **permission scope** (a bitmask) and a **responsibilities commitment** (a SHA-256 hash of the off-chain responsibilities text). Other team contracts — invitations (CAPSTONE-042), acceptance (CAPSTONE-043), and conflict checks (CAPSTONE-045) — read the role and its scope from here. They do not define roles of their own.

The contract does not judge contribution quality, grade work, or store task assignments. It answers two questions: "which role does this address hold on this project?" and "does that role grant permission X?"

## Role model

Built-in role kinds have stable discriminants. Never renumber them.

| Kind | Value | Default scope |
|------|-------|---------------|
| `Lead` | 0 | `MANAGE_TEAM \| MANAGE_ROLES \| SUBMIT \| EDIT \| VIEW` |
| `Researcher` | 1 | `EDIT \| VIEW` |
| `Developer` | 2 | `EDIT \| VIEW` |
| `Designer` | 3 | `EDIT \| VIEW` |
| `Analyst` | 4 | `EDIT \| VIEW` |
| `Writer` | 5 | `EDIT \| VIEW` |
| `Reviewer` | 6 | `REVIEW \| VIEW` |
| `Custom` | 7 | none — the scope must be set explicitly |

Permission bits (`u32`; unknown bits are rejected with `InvalidScope`):

| Bit | Name | Meaning |
|-----|------|---------|
| 0 | `VIEW` | Read project references and team roster |
| 1 | `EDIT` | Contribute artifacts (off-chain; the on-chain effect is commitment updates) |
| 2 | `SUBMIT` | Submit deliverable commitments for review |
| 3 | `REVIEW` | Record review attestations; mutually exclusive with `SUBMIT` on the same project |
| 4 | `MANAGE_TEAM` | Issue/revoke invitations (CAPSTONE-042) |
| 5 | `MANAGE_ROLES` | Define custom roles and reassign roles |

A `RoleDef` is `{ project_id: BytesN<32>, role_id: u32, kind: RoleKind, scope: u32, responsibilities_hash: BytesN<32>, version: u32, created_at: u64 }`. The built-in kinds use `role_id` values 0–6, and custom roles start at 100. Custom role labels and responsibility text stay off-chain. Only the hash is stored.

A `RoleBinding` is `{ project_id, member: Address, role_id, role_version: u32, bound_at: u64 }`. The pinned `role_version` lets consumers detect that a binding refers to a superseded role definition (see stale transitions below).

## Lifecycle and transitions

1. **Initialize project roles.** `init_project(authority, project_id)` checks that `project_id` is registered in `capstone-identifiers`, that `authority` is that identity's authority, and `authority.require_auth()`. It writes the seven built-in `RoleDef`s at version 1. Calling it again returns `AlreadyInitialized`.
2. **Define a custom role.** `define_role(caller, project_id, scope, responsibilities_hash)` requires `MANAGE_ROLES` and returns a new `role_id`. A project can hold at most `MAX_CUSTOM_ROLES = 16` custom roles.
3. **Revise a role.** `revise_role(caller, project_id, role_id, expected_version, scope, responsibilities_hash)` bumps `version`. If `expected_version` is not the current version, it returns `StaleVersion`. A `Lead` role can never lose `MANAGE_ROLES`.
4. **Bind a role.** `bind(caller, project_id, member, role_id)` can be called only by the acceptance contract (CAPSTONE-043) during activation, or by a `MANAGE_ROLES` holder on an already-active team. Each member holds at most one role per project.
5. **Reassign a role.** `reassign(caller, project_id, member, expected_role_id, new_role_id)` uses compare-and-swap on the current binding.
6. **Unbind a role.** `unbind(caller, project_id, member)` can be called by a `MANAGE_ROLES` holder or by the member themselves (voluntary exit). If it would remove the last `Lead`, it returns `LastLead`.
7. **Retire a role.** `retire_role(caller, project_id, role_id)` makes the role unbindable. If members still hold it, it returns `RoleInUse`.

Read methods are `get_role`, `get_binding`, `has_permission(project_id, member, perm_bit) -> bool`, `roster(project_id) -> Vec<RoleBinding>` (at most 32 entries), and `storage_version()`.

## Authorization

Authorization is deny-by-default. A method with no binding for the caller returns `NotMember`, and a binding without the needed bit returns `MissingPermission`. Every mutating call runs `require_auth()` on the acting address. The cross-contract caller allowlist (the acceptance contract address) is set once by the admin at `initialize` and can be changed only through an admin call that emits `ROLECFG`.

**Tenant boundary:** each project's `capstone-identifiers` record names its authority (the tenant). Managing roles requires a binding on that same `project_id`. Holding `MANAGE_ROLES` on another project grants nothing.

**Separation of duties:** if a bind or reassign would give a member both `SUBMIT` and `REVIEW` on the same project, the contract returns `SeparationOfDuties`. Deeper conflict checks (supervision, grading, and so on) belong to CAPSTONE-045.

## Idempotency and replay

- Binding a member to the role they already hold, at the current role version, is a no-op. It returns `Ok` and emits no event, so retries never duplicate roster entries.
- Revisions, reassignments, and role definitions use `expected_version` or `expected_role_id` compare-and-swap. A replayed transaction with an old expectation fails with `StaleVersion` or `StaleBinding` and does not overwrite newer state.
- The Soroban auth nonce protects each signed invocation against replay. The contract adds no nonces of its own.

## On-chain vs off-chain

| On-chain | Off-chain (other repos) |
|----------|-------------------------|
| `project_id`, `role_id`, kind, scope bits, responsibilities hash, versions, member addresses, timestamps | Role names for custom roles, responsibilities text, task boards, contribution logs, member profiles |

Responsibility documents must be canonicalized (UTF-8 NFC, LF line endings) before hashing, so clients can re-verify them. Member addresses are pseudonymous, but they are still linkable. Clients must not publish any address-to-person mapping.

## Storage bounds and TTL

- `RoleDef(project_id, role_id)`: 7 built-in entries plus up to 16 custom entries per project.
- `Binding(project_id, member)`: one entry per member. `Roster(project_id)` holds at most `MAX_TEAM_SIZE = 32` addresses (`TeamFull`).
- Every entry is fixed-size. The persistent TTL follows the capstone defaults (`RECORD_MIN_TTL = 3_110_400`, `RECORD_MAX_TTL = 6_220_800`) and is extended on read and on write. A keeper must renew rosters for long-running projects.

## Events

All topics are ≤ 9 characters (`symbol_short!`). Payloads contain no private text.

| Topic | Data |
|-------|------|
| `ROLEINIT` | `(project_id)` |
| `ROLEDEF` | `(project_id, role_id, kind, scope, responsibilities_hash, version)` |
| `ROLEBIND` | `(project_id, member, role_id, role_version)` |
| `ROLEMOVE` | `(project_id, member, old_role_id, new_role_id)` |
| `ROLEUNB` | `(project_id, member, role_id)` |
| `ROLERET` | `(project_id, role_id)` |
| `ROLECFG` | `(acceptance_contract)` |

## Errors (stable codes)

| Code | Name | Caller action |
|------|------|---------------|
| 1 | `AlreadyInitialized` | Read the existing roles instead of re-initializing |
| 2 | `NotInitialized` | Call `init_project` first |
| 3 | `ProjectNotFound` | Register the project in `capstone-identifiers` |
| 4 | `Unauthorized` | Sign as the project authority or the allowlisted contract |
| 5 | `NotMember` | The caller must hold a role on this project |
| 6 | `MissingPermission` | Ask a Lead to grant the needed scope |
| 7 | `InvalidScope` | Remove unknown bits |
| 8 | `RoleNotFound` | Check `role_id` |
| 9 | `RoleRetired` | Choose an active role |
| 10 | `RoleInUse` | Reassign the holders before retiring the role |
| 11 | `StaleVersion` | Re-read the role and retry with the current version |
| 12 | `StaleBinding` | Re-read the binding and retry |
| 13 | `TooManyRoles` | Retire an unused custom role |
| 14 | `TeamFull` | The team is at capacity |
| 15 | `LastLead` | Assign another Lead first |
| 16 | `SeparationOfDuties` | Do not combine SUBMIT and REVIEW |

## Required tests

- **Authorization tree:** for each mutating method, cover no auth, the wrong project, a member without the permission bit, a member with it, and the allowlisted contract versus an arbitrary contract.
- **Replay:** replay `revise_role` and `reassign` with stale expectations. A duplicate `bind` must be a no-op with zero events.
- **State transitions:** retire while in use, remove the last Lead, strip `MANAGE_ROLES` from Lead, and the SUBMIT+REVIEW conflict.
- **Bounds:** the 17th custom role and the 33rd member fail. Measure the CPU and memory budget of `roster()` at 32 members against a recorded ceiling.
- **Events:** assert the exact topic and data tuple for each event.
- **Errors:** pin every discriminant (a test that asserts `as u32` values).
- **Upgrade compatibility:** assert `storage_version() == 1` and decode a persisted `RoleDef`/`RoleBinding` fixture after a WASM upgrade.

## Dependencies

- `capstone-identifiers`: checks that the project exists and finds its authority.
- CAPSTONE-043 `capstone-team-acceptance`: the only automated caller of `bind`.
- CAPSTONE-042 and CAPSTONE-045: read `get_role` and `has_permission`.

## Rollout and rollback

**Rollout:**
1. Deploy and `initialize(admin)`.
2. Set the acceptance-contract allowlist once CAPSTONE-043 is deployed.
3. Call `init_project` for each project as it is created. The contract starts at schema version 1 and needs no migration.

**Rollback:** stop routing writes to the contract (gated by a feature flag off-chain). On-chain bindings stay readable. Do not delete them, because downstream attestations reference `role_version`. Make schema upgrades additive only.

## Corresponding CAPSTONE-044 work in other repositories

- **Backend:** the custom-role CRUD API, canonical responsibilities documents and hashing, mapping permission bits to API guards, and an indexer projection of the `ROLE*` events.
- **Frontend:** the role picker, responsibilities editor with hash display and verification, and team roster view.
- **Off-chain policy:** the tenant-level catalogue of default responsibilities for each built-in kind.
