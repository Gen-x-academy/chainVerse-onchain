# CAPSTONE-065: Create Project Collaboration Workspace

**Track:** Workspace | **Component:** on-chain | **Issue:** #1413 | **Status:** implemented
**Audit baseline:** `main` @ `8713c23` (2026-11-22).
**Contract:** `contracts/capstone-project-workspace`

## Scope

Give each active project one scoped home with an overview, members, objectives, milestones,
resources, activity, and next actions. The chain is the authoritative access list and a versioned
commitment registry: it stores who may enter, what they may edit, the lifecycle state, and one
commitment per section. Section content, rosters with display names, and feeds live off-chain.

## On-chain stores vs off-chain content

| On-chain store | Contents |
| --- | --- |
| `Admin` (instance) | One-time deployment administrator. |
| `Role(account, tenant, project, role)` | `RoleGrant { active, expires_at }` for `Manager`. |
| `Request(tenant, project, request_id)` | Idempotency record: input commitment and recorded result. |
| `Workspace(tenant, project)` | State, member count, timestamps, workspace version. |
| `Member(tenant, project, account)` | Member role (`Lead`, `Contributor`, `Observer`), `added_at`, `expires_at`. |
| `MemberList(tenant, project)` | Ordered account list, capped at 64. |
| `Section(tenant, project, section)` | `SectionRef`: `ref_hash`, section version, `updated_at`. |

## Authorization

Managers (granted by the administrator per tenant and project) provision the workspace, manage
membership, and change lifecycle state. Members read the workspace and edit sections by role.

| Section | Manager | Lead | Contributor | Observer |
| --- | --- | --- | --- | --- |
| `Overview`, `Objectives`, `Milestones` | edit | edit | read | read |
| `Members` (roster commitment) | edit | read | read | read |
| `Resources`, `Activity`, `NextActions` | edit | edit | edit | read |

Reads (`get_home`, `get_section`, `get_member`, `list_members`) require a manager role or an
unexpired membership. Strangers, other tenants, and other projects get `Unauthorized`; expired
memberships and roles get `RoleExpired`. Membership comes only from `add_member`, which only a manager
can call, so a member can never grant access to anyone else.

## Lifecycle

| From | Allowed targets |
| --- | --- |
| `Active` | `Paused`, `Archived` |
| `Paused` | `Active`, `Archived` |
| `Archived` | none (terminal) |

While `Paused`, data stays readable but section edits and member additions return `InvalidState`;
member removal is still allowed. `Archived` freezes everything except reads. `set_state` takes the
current workspace `version`; because the version moves on every mutation (membership, section edits,
state), a stale caller gets `StaleVersion`, and same-state or terminal moves get `InvalidTransition`.

## Versioning

The workspace version starts at 1 on creation and increments on every mutation. Each section has its
own version, starting at 0 when unset, so the first write passes `expected_section_version = 0` and a
concurrent writer holding an old version gets `StaleVersion`. `update_section` returns the section
version; the other mutations return the new workspace version.

## Idempotency and duplicate protection

Mutations supply `request_id`; the commitment binds domain, tenant, project, request id, actor, and all
fields. Replays return the recorded result with no write and no event. A different payload or actor
under the same request id returns `RequestConflict`. A second `add_member` for an existing account
returns `AlreadyMember` and a second `create_workspace` returns `AlreadyExists`, so concurrent
requests cannot duplicate membership or the home. Removed members can be added again.

## Validation and bounds

64 members per workspace (`LimitReached`), pages of at most 32 (`InvalidPage`). A membership
`expires_at` is 0 (none) or in the future and at most five years ahead. Section and overview hashes
must be non-zero salted commitments. TTLs extend on write and on most reads.

## Errors

Append-only codes: `1 AlreadyInitialized`, `2 NotInitialized`, `3 Unauthorized`, `4 RequestConflict`,
`5 StaleVersion`, `6 NotFound`, `7 AlreadyExists`, `8 InvalidInput`, `9 InvalidTransition`,
`10 LimitReached`, `11 InvalidPage`, `12 ArithmeticError`, `13 RoleExpired`, `14 InvalidState`,
`15 AlreadyMember`.

## Events

| Topic | Payload |
| --- | --- |
| `WSP_INIT` | `(admin)` |
| `WSP_ROLE` | `(tenant, project, account, role, active, expires_at)` |
| `WSP_NEW` | `(tenant, project, version)` |
| `WSP_MEM` | `(tenant, project, member, member_role, expires_at, version)` |
| `WSP_RMV` | `(tenant, project, member, version)` |
| `WSP_SEC` | `(tenant, project, section, section_version, version)` |
| `WSP_STS` | `(tenant, project, from, to, version)` |

No overview text, resource names, activity entries, or next-action text appear in events.

## Privacy boundaries and off-chain boundary

Member addresses are stored to enforce access and are public ledger data. Display names, avatars, and
contact details stay in the identity service. The `Members` section commitment represents the
off-chain roster view; the on-chain `Member` entries remain the access authority. Section content is
fetched from the workspace service, which must check the caller against `get_member` or `get_home`
before serving it. Ledger storage is public, so access control here governs the contract API and
service authorization, not secrecy of the committed hashes.

## Dependencies

`capstone-identifiers` for project ids. Objectives, milestones, resources, activity, and next actions
are produced by `capstone-objectives` (CAPSTONE-057), `capstone-project-milestones` (CAPSTONE-060),
`capstone-resource-library` (CAPSTONE-068), `capstone-activity-feed` (CAPSTONE-072), and the service.
Pause and archive states should be mirrored from the pause and closure work (CAPSTONE-053, CAPSTONE-054).

## Rollout and rollback

Deploy, initialize, grant manager roles, create each project's workspace, add members, then enable
the workspace UI. Existing projects backfill by creating the workspace and re-adding members. Rollback:
stop invoking mutating entry points; reads remain valid; pause or archive individual workspaces to
freeze them without deleting anything.

## Upgrade compatibility

Error codes, enum discriminants (`Role`, `MemberRole`, `WorkspaceState`, `Section`), and `DataKey`
variants are append-only and snapshot-tested. A stored-struct field change is not decode-compatible
and must ship as a new `DataKey` variant with a migration. A test writes `Workspace`, `Section`,
`Member`, `Role`, and `Request` records directly to prove a newer build reads and continues them.

## Test coverage map

Home provisioning and uniqueness; per-section authorization for every role; read access rules;
tenant and project isolation; duplicate membership and the 64-member bound with paging; replay and
conflict; stale section versions; the full lifecycle including paused and archived behaviour; member
removal and re-addition; membership and manager expiry; event schemas for every topic; error and enum
snapshots; legacy decode; CPU and memory budgets for edit, add, and remove.

## Corresponding work in other repositories (CAPSTONE-065)

- **Backend/workspace-service:** section content storage, roster display data, access checks against this contract, activity aggregation.
- **Frontend:** workspace home with the seven sections, member management, paused and archived banners.
- **Mobile:** read-only home and next actions through the service.
- **Docs/indexer:** projection of `WSP_*` events and workspace version history.
