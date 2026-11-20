# CAPSTONE-052: Amend Active Project Charters

**Track:** Governance | **Component:** on-chain | **Issue:** #1400 | **Status:** specified (not yet implemented)

Goal: Amend active project charters.

## Baseline

Revalidated against `main` at `4471755`. There is no charter registry yet. This
work extends `contracts/capstone-charters`, which is introduced by
CAPSTONE-050, so it has to land after or together with that issue.

## On-chain

An amendment is a proposal to replace the `Accepted` charter version `n` with
version `n+1`. Version `n` is never modified. When the amendment takes effect,
version `n` becomes `Superseded` and version `n+1` becomes current.

### Data model

| Type | Fields | Notes |
| --- | --- | --- |
| `Amendment` | `project_id`, `base_version: u32`, `new_version: u32`, `new_charter_hash`, `impact_hash: BytesN<32>`, `impact_flags: u32`, `material: bool`, `required_count: u32`, `proposed_by`, `proposed_at`, `effective_at: u64`, `decide_by: u64`, `state: AmendmentState` | |
| `AmendmentState` | `Proposed`, `Approved`, `Effective`, `Rejected`, `Withdrawn`, `Expired` | |

`impact_flags` bits: `SCOPE = 1`, `TIMELINE = 2`, `FUNDING = 4`,
`MEMBERSHIP = 8`, `IP_DATA = 16`. Setting `SCOPE`, `FUNDING` or `IP_DATA`
automatically sets `material = true`.

`impact_hash` commits to the impact preview generated off-chain (a diff
summary, the timeline shift, budget changes), so every approver signs the same
preview.

Only one amendment can be open per project (`OpenAmendment(project_id)`).

### Entry points

- `propose_amendment(proposer, project_id, base_version, new_charter_hash, impact_hash, impact_flags, effective_at, decide_by)`:
  the proposer must be a signer of `base_version` or a tenant `Coordinator`.
  The required approvals come from policy: a material amendment needs every
  signer of the base version, and a non-material one needs the base version's
  `required_count`.
- `approve_amendment(actor, project_id, new_version, new_charter_hash, impact_hash, intent_nonce)`:
  the actor signs both hashes.
- `reject_amendment(actor, project_id, new_version)`: any required signer can
  reject. A single rejection ends the amendment.
- `withdraw_amendment(proposer, project_id, new_version)`: allowed only while
  the amendment is `Proposed`.
- `activate_amendment(caller, project_id, new_version)`: anyone can call it
  once the amendment is `Approved` and `now >= effective_at`. It is
  idempotent.
- `get_amendment`, `get_charter_history(project_id) -> Vec<(u32, BytesN<32>, CharterState)>`.
  The history is capped at `MAX_VERSIONS = 32`.

### Lifecycle

```
Proposed ──all approvals──▶ Approved ──now ≥ effective_at, activate──▶ Effective
   │                            │                                          │
   ├──reject──▶ Rejected        └──base no longer current──▶ Expired       └─ base → Superseded
   ├──withdraw──▶ Withdrawn
   └──now > decide_by──▶ Expired
```

On `Effective`:
- version `n+1` is written as an `Accepted` `CharterVersion`, carrying the
  approvals as its acceptances;
- if the amendment is `material`, every guardian consent tied to version `n`
  moves to `ReconsentRequired` (CAPSTONE-051) the next time it is checked;
- the `CHRAMEFF` event lets the backend reschedule deadlines and funding.

## Authorization and tenant boundaries

- Deny by default: approvers are the signer set of `base_version`, and a
  proposal from outside the tenant fails with `WrongTenant`.
- The proposer's own approval is not implied. They have to call
  `approve_amendment` explicitly.
- Coordinators can propose, but they can approve only if they are in the signer
  set.

## Replay and stale transitions

- `base_version` must equal `Current(project_id)`, otherwise the call fails with
  `StaleBase`. Two concurrent proposals can't both succeed: the second one gets
  `AmendmentAlreadyOpen`.
- An approval must match both `new_charter_hash` and `impact_hash`
  (`StaleAmendment`), so changing the preview after people have approved it
  invalidates those approvals.
- Nonces are single-use. Approving twice returns `AlreadyApproved`.
- Calling `activate_amendment` again on an `Effective` amendment returns `Ok`
  without emitting another event.

## Off-chain boundary

| Stays off-chain | On-chain |
| --- | --- |
| Amended charter text and redline diff | `new_charter_hash` |
| Impact preview content (dates, budget numbers, member names) | `impact_hash` plus `impact_flags` |
| Rationale and discussion | Nothing on-chain |
| Rescheduling and notifications | Driven by events |

## Events and errors

| Event | Topics | Data |
| --- | --- | --- |
| `CHRAMPRP` | `(project_id,)` | `(base_version, new_version, impact_flags, material, effective_at)` |
| `CHRAMAPR` | `(project_id, new_version)` | `(actor,)` |
| `CHRAMREJ` | `(project_id, new_version)` | `(actor,)` |
| `CHRAMWD` | `(project_id, new_version)` | `()` |
| `CHRAMEFF` | `(project_id,)` | `(base_version, new_version, material)` |

These extend the CAPSTONE-050 `ContractError` enum and continue its numbering:
`AmendmentNotFound = 15`, `AmendmentAlreadyOpen = 16`, `StaleBase = 17`,
`StaleAmendment = 18`, `AlreadyApproved = 19`, `AmendmentNotOpen = 20`,
`NotEffectiveYet = 21`, `InvalidEffectiveDate = 22`, `DecisionWindowClosed = 23`,
`InvalidImpactFlags = 24`, `HistoryFull = 25`, `BaseNotAccepted = 26`.

## Required tests

- Authorization: a non-signer approving, a cross-tenant proposer, and a material
  amendment that is still missing one signer.
- Replay and concurrency: two proposals against the same base, an approval with
  an old `impact_hash`, a reused nonce, and calling activate twice.
- Transitions: every edge in the diagram. Version `n` must be byte-identical
  before and after activation, apart from its `state`.
- Expiry: `decide_by` and `effective_at` boundaries, each tested at `t`
  and `t + 1`.
- Storage bounds: `MAX_VERSIONS` reached, giving `HistoryFull`.
- Event snapshots, error codes, the resource budget for `activate_amendment`
  with 16 signers, and upgrade compatibility with charters written under
  CAPSTONE-050.

## Dependencies, rollout and rollback

- Depends on CAPSTONE-050 (same crate). Interacts with CAPSTONE-051 (re-consent)
  and CAPSTONE-053 (an amendment can't be proposed while the project is
  `Paused`, and the call fails with `ProjectPaused` from the pause contract).
- Rollout: ship with CAPSTONE-050 or in the release right after it. Adding
  amendment storage is a schema addition and doesn't change existing records.
- Rollback: withdraw any open amendments. Versions that are already effective
  stay on-chain as immutable history. The backend has to keep reading
  `Current(project_id)` as the source of truth.
- Other repositories: the backend builds the impact preview, computes the
  hashes and does the rescheduling. The frontend shows the redline view, the
  approval inbox and the version history.
