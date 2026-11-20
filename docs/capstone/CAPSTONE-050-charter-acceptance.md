# CAPSTONE-050: Record Charter Acceptance

**Track:** Governance | **Component:** on-chain | **Issue:** #1398 | **Status:** specified (not yet implemented)

Goal: Record charter acceptance.

## Baseline

Revalidated against `main` at `4471755` (audit baseline `5b62b36`, 2026-11-18).
`main` has `capstone-identifiers` (public project ids), `capstone-call-terms`,
`capstone-readiness`, `capstone-delivery-modes`, `capstone-shortlists`,
`capstone-opportunity-details` and `capstone-profile-visibility`. None of them
stores charters or approvals, so this work adds a new primitive.

## On-chain

New crate `contracts/capstone-charters`. It keeps a versioned registry of
charter *commitments* for each project, plus an acceptance ledger. CAPSTONE-052
(amendments) extends the same crate.

### Data model

| Type | Fields | Notes |
| --- | --- | --- |
| `CharterVersion` | `tenant_id: BytesN<32>`, `project_id: BytesN<32>`, `version: u32`, `charter_hash: BytesN<32>`, `required_signers_hash: BytesN<32>`, `required_count: u32`, `published_at: u64`, `acceptance_deadline: u64`, `state: CharterState` | `project_id` is the public id from `capstone-identifiers`. The charter text is never stored. |
| `Acceptance` | `actor: Address`, `role: Role`, `charter_hash`, `version`, `intent_nonce: BytesN<32>`, `accepted_at: u64`, `expires_at: u64` | One per `(project_id, version, actor)`. |
| `CharterState` | `Draft`, `Open`, `Accepted`, `Expired`, `Superseded` | |
| `Role` | `Student`, `Supervisor`, `Sponsor`, `Coordinator` | `Guardian` approvals go through CAPSTONE-051. |

Storage keys: `Charter(project_id, version)`, `Current(project_id)`,
`Acceptance(project_id, version, actor)`, `AcceptCount(project_id, version)`,
`Nonce(actor, intent_nonce)`, `Signer(project_id, version, actor) -> Role`.
All are persistent with explicit TTL extension, matching `capstone-identifiers`
(`3_110_400` / `6_220_800` ledgers).

### Entry points

- `publish_charter(coordinator, tenant_id, project_id, charter_hash, signers: Vec<(Address, Role)>, required_count, acceptance_deadline)`
  creates version `n+1` in `Open`. The coordinator must hold the `Coordinator`
  role for `tenant_id`. The signer list is capped at `MAX_SIGNERS = 16`.
- `accept_charter(actor, project_id, version, charter_hash, intent_nonce, expires_at)`
  records an explicit approval. It calls `actor.require_auth()`, and the signed
  arguments bind the actor to the exact `charter_hash` and `version`, so an
  approval cannot be replayed against a different version.
- `revoke_acceptance(actor, project_id, version)` is allowed only while the
  charter is still `Open`.
- `get_charter`, `get_current_version`, `get_acceptance`, `is_fully_accepted`
  are read-only.

### Lifecycle

```
Draft ──publish──▶ Open ──required_count reached──▶ Accepted
                    │                                   │
                    ├──deadline passes──▶ Expired       └──amendment effective (052)──▶ Superseded
                    └──new version published──▶ Superseded
```

- Reaching `required_count` distinct, eligible, unexpired acceptances moves the
  version to `Accepted` in the same invocation.
- An individual acceptance whose `expires_at` has passed stops counting. If the
  count then drops below the threshold before the charter is `Accepted`, the
  actor has to accept again with a fresh nonce.
- Once a version is `Accepted`, its record cannot be changed.

## Authorization and tenant boundaries

- Deny by default: only addresses in the signer set for that version can
  accept, and only with the role they were assigned.
- `tenant_id` is fixed when the charter is published. A coordinator from
  another tenant gets `WrongTenant`.
- There is no admin override for accepting on someone's behalf.

## Replay and idempotency

- `Nonce(actor, intent_nonce)` is single-use. Reusing it returns `NonceUsed`.
- A second acceptance by the same actor for the same version returns
  `AlreadyAccepted` and leaves the first record and the count unchanged. A
  client retry after a timeout is therefore safe: the client reads
  `get_acceptance` and treats `AlreadyAccepted` as success.
- If the client's `charter_hash` does not match the stored hash, the call fails
  with `StaleCharter`, so a client that rendered an outdated charter cannot
  accept the newer one by mistake.

## Off-chain boundary

| Stays off-chain (capstone service) | On-chain |
| --- | --- |
| Charter text, scope, deliverables, IP and data terms | `charter_hash` (SHA-256 over the canonical JSON) |
| Participant names, emails, institution records | Stellar `Address` and `Role` only |
| Full signer roster with display data | `required_signers_hash` plus the address set |
| Rendering, e-sign UX, receipts, notifications | Events only |

The service canonicalizes the charter and computes the hash, and the client
verifies that hash before it asks the user to sign. Events never contain free
text.

## Events and errors

| Event | Topics | Data |
| --- | --- | --- |
| `CHRPUB` | `(project_id,)` | `(version, charter_hash, required_count)` |
| `CHRACC` | `(project_id, version)` | `(actor, role)` |
| `CHRREV` | `(project_id, version)` | `(actor,)` |
| `CHRDONE` | `(project_id,)` | `(version,)` |

`ContractError` codes:
`Unauthorized = 1`, `WrongTenant = 2`, `CharterNotFound = 3`,
`StaleCharter = 4`, `NotASigner = 5`, `RoleMismatch = 6`,
`AlreadyAccepted = 7`, `NonceUsed = 8`, `CharterNotOpen = 9`,
`AcceptanceWindowClosed = 10`, `InvalidExpiry = 11`, `TooManySigners = 12`,
`InvalidThreshold = 13`, `VersionOverflow = 14`.
These codes are stable. New variants go at the end and existing codes are never
renumbered.

## Required tests

- Authorization: signer vs non-signer, wrong role, cross-tenant coordinator, and
  a missing `require_auth` (using `mock_auths` with explicit invocations, not
  `mock_all_auths`).
- Replay: reused nonce, same actor twice, and acceptance against an old
  `charter_hash` or version.
- Transitions: `Open → Accepted` exactly at the threshold, `Open → Expired` at
  `deadline + 1`, and the rule that an `Accepted` version is never changed.
- Expiry: an acceptance that expires before the threshold is reached is not
  counted.
- Storage bounds: `MAX_SIGNERS` and `MAX_SIGNERS + 1`.
- Event schema snapshots and error-code value assertions.
- Resource budget: `accept_charter` with 16 signers stays inside the Soroban
  CPU and memory limits (`env.cost_estimate()`).
- Upgrade compatibility: records written by v1 are still readable after a WASM
  upgrade.

## Dependencies, rollout and rollback

- Depends on `capstone-identifiers` for `project_id`, and on the tenant role
  registry (shared access control) for the `Coordinator` role.
- Rollout: deploy the contract to testnet behind the capstone feature flag, and
  have the service dual-write (database plus chain) for one cohort before the
  chain becomes authoritative.
- Rollback: the service stops calling the contract. Existing records stay
  readable, and because none of them hold private data, nothing needs to be
  purged.
- Other repositories: the backend handles charter canonicalization, hashing,
  signer roster and notifications. The frontend handles the charter review
  screen, the signing flow and the acceptance status.
