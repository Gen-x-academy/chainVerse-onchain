# CAPSTONE-045: Team Conflict-of-Interest Detection

> Issue: #1393 · Track: Teams · Status: **specification** — the `capstone-team-conflicts` contract is not yet implemented. Revalidated against `main` at `4471755` (2026-11-20); the audited baseline was `5b62b369`. This contract follows the pattern of the existing `mentorship-conflicts` contract but is scoped to capstone teams and reviewers.

## Contract boundary

`capstone-team-conflicts` is a registry of conflict-of-interest relationships, together with a **gate**. Before a team activates (CAPSTONE-043) and before a reviewer or grader is assigned, the caller asks this contract whether any registered relationship links the parties. If one does, the gate refuses the transition with a stable error.

The contract stores only that a relationship of a given type exists between two pseudonymous addresses, plus a commitment to the supporting evidence. It never stores what the relationship is in detail: no employer names, family ties, contract amounts, or narrative.

The contract does **not** discover conflicts on its own. Relationships are declared by the parties or attested by an authorized compliance attester, off the back of off-chain checks. The gate is only as complete as the declarations and attestations it holds. The documentation and the UI must say so.

## Relationship model

```
enum RelationType {
  Supervision = 0,  // A supervises/advises B outside this project
  Grading     = 1,  // A grades/assesses B in a concurrent course
  Employment  = 2,  // A employs B or they share an employer/reporting line
  Sponsor     = 3,  // A represents a sponsor funding B or B's project
  Financial   = 4,  // material financial interest between A and B
  Family      = 5,  // family or close personal relationship
  Declared    = 6,  // any other self-declared conflict
}

Relationship {
  rel_id:        BytesN<32>,  // sha256(tenant ‖ min(a,b) ‖ max(a,b) ‖ type)
  tenant:        Address,     // institution authority from capstone-identifiers
  party_a:       Address,     // canonical ordering: party_a < party_b
  party_b:       Address,
  rel_type:      RelationType,
  source:        Source,      // SelfDeclared = 0 | Attested = 1
  evidence_hash: BytesN<32>,  // commitment to the off-chain record; zero for none
  valid_until:   u64,         // 0 = no expiry; otherwise ledger timestamp
  recorded_at:   u64,
  revoked:       bool,
}
```

Relationships are symmetric and **tenant-scoped**: a relationship recorded under tenant X has no effect on projects belonging to tenant Y. The pair is stored in canonical address order, so (A,B) and (B,A) map to the same `rel_id` and cannot be recorded twice.

## Operations

- `declare(party, counterparty, project_id, rel_type, evidence_hash, valid_until)`: either party self-declares a relationship. The `party` must authorize the call and the tenant is derived from `project_id`. A self-declaration can only **add** a conflict. Declaring a conflict can never unblock anything, so the call needs no further approval.
- `attest(attester, tenant, party_a, party_b, rel_type, evidence_hash, valid_until)`: a compliance attester registered for that tenant records a relationship found through off-chain checks, such as HR, sponsor, or family-disclosure data.
- `revoke(caller, rel_id, reason_hash)`: an `Attested` relationship can be revoked only by a tenant attester. A `SelfDeclared` one can be revoked by the declarer or a tenant attester. Revocation is soft: the record stays for audit with `revoked = true`.
- `check_team(project_id, members: Vec<Address>, supervisors: Vec<Address>) -> Result<(), ContractError>`: the activation gate. It checks every member × supervisor pair and every member × member pair for active relationships of the `Supervision`, `Grading`, `Employment`, `Sponsor`, `Financial`, or `Family` types, and of `Declared` type for any pair. It returns `ConflictDetected` on the first hit. It is read-only and emits nothing.
- `check_assignment(project_id, reviewer, team: Vec<Address>) -> Result<(), ContractError>`: the review-assignment gate. It checks the reviewer against every team member for any active relationship type.
- `record_check(caller, project_id, subject_hash, outcome)`: lets an allowlisted gate caller (CAPSTONE-043 or the review-assignment flow) persist the outcome together with a `subject_hash = sha256(sorted party list)`. A private conflict is not echoed in the public audit trail, because the outcome is recorded as passed or blocked only, with no relationship type.

A relationship is **active** when `!revoked && (valid_until == 0 || now < valid_until)`. Expired relationships are ignored without any error.

### Rules for which types block which checks

| Type | Team activation (member↔supervisor) | Team activation (member↔member) | Reviewer assignment |
|------|----|----|----|
| Supervision | block | allow (peer mentoring is fine) | block |
| Grading | block | allow | block |
| Employment | block | allow | block |
| Sponsor | block | allow | block |
| Financial | block | block | block |
| Family | block | block | block |
| Declared | block | block | block |

Tenants cannot relax these rules on-chain. A tenant that needs a policy exception must handle it off-chain by revoking or expiring the relationship through an attester, which leaves an audit record.

## Authorization and tenant boundary

Authorization is deny-by-default:
- `declare` requires the declaring party's auth.
- `attest` and attester-revoke require an address listed in `Attester(tenant, address)`. Only the tenant authority can add or remove attesters, and it must authorize the call.
- `record_check` accepts only allowlisted contract callers.
- The `check_*` functions are public reads. Their output is only pass, or a block naming the first conflicting pair, never the type. Exposing the type would leak sensitive facts, such as a family tie, to anyone who can call the gate.

Every call resolves the tenant from `project_id` through `capstone-identifiers`. A relationship's tenant must match the project's tenant for it to count.

## Privacy

| On-chain | Off-chain only |
|----------|----------------|
| Address pair, relationship type, source, evidence hash, validity window, revocation flag | Employer names, sponsor contracts and amounts, family details, HR records, disclosure narratives, reviewer rationale |

Relationship types are stored in contract storage. Contract storage is publicly readable on Stellar, so anyone who reads it can see that two addresses share, for example, a `Family` link. This is the minimum the gate needs to stay deterministic. To limit the exposure:
1. Events never include `rel_type` (see below).
2. Clients must never publish address-to-identity mappings.
3. Tenants who need stronger privacy can set every relationship to `Declared` (`rel_type = 6`), which blocks every check. This gives up type-specific rules in exchange for concealing the relationship type.

`evidence_hash` must commit to a salted record (`sha256(salt ‖ canonical_record)`), so that low-entropy evidence cannot be brute-forced.

## Idempotency and replay

- `rel_id` is deterministic, so declaring or attesting the same pair and type again **updates** `evidence_hash` and `valid_until` in place (attested wins over self-declared) and never creates a second record. Re-declaring identical values is a no-op with no event.
- `revoke` on a relationship that is already revoked returns `Ok` with no event.
- `record_check` is keyed by `(project_id, subject_hash)` and the last write wins. Its outcome is advisory audit data. The authoritative gate is the synchronous `check_*` call inside the activation transaction, so a stale recorded check can never authorize anything.

## Storage bounds and TTL

- `Relation(rel_id)` is fixed-size.
- `PartyIndex(tenant, address)` holds at most `MAX_RELATIONS_PER_PARTY = 32` ids. When it is full, the call returns `TooManyRelations` and the attester must consolidate.
- The `check_team` input is limited to 32 members and 8 supervisors, and `check_assignment` to 32 team members. The worst case is 752 pair lookups (32×8 + C(32,2) = 256 + 496), and `PartyIndex` scans keep each lookup O(32). That figure must be measured against the Soroban per-transaction budget and pinned in a test. If the budget cannot hold it, lower the bounds before release.
- TTL follows the capstone defaults. An expired `valid_until` does not delete the record.

## Events

Event payloads deliberately omit `rel_type` and the evidence hash.

| Topic | Data |
|-------|------|
| `COIREC` | `(rel_id, tenant, source)` |
| `COIUPD` | `(rel_id)` |
| `COIREV` | `(rel_id, revoked_by)` |
| `COIATT` | `(tenant, attester, added: bool)` |
| `COICHK` | `(project_id, subject_hash, passed: bool)` |

## Errors (stable codes)

| Code | Name | Caller action |
|------|------|---------------|
| 1 | `NotInitialized` | Initialize the contract |
| 2 | `AlreadyInitialized` | — |
| 3 | `Unauthorized` | Sign with the correct party, attester, or tenant |
| 4 | `ProjectNotFound` | Register the project |
| 5 | `TenantMismatch` | The relationship or project belongs to a different tenant |
| 6 | `SelfRelation` | The parties must differ |
| 7 | `InvalidRelationType` | Use a defined type |
| 8 | `InvalidValidity` | `valid_until` must be 0 or in the future |
| 9 | `RelationNotFound` | Check `rel_id` |
| 10 | `ConflictDetected` | Resolve off-chain: choose a different supervisor/reviewer or obtain an attested revocation |
| 11 | `TooManyRelations` | An attester must consolidate the party's relationships |
| 12 | `InputTooLarge` | Split the request or reduce the team, supervisor, or reviewer set |
| 13 | `NotAllowlisted` | Only gate contracts may record checks |

## Required tests

- **Authorization tree:** declare by a non-party, attest by a non-attester or an attester of another tenant, revoke an attested relationship by its declarer (must be denied), and `record_check` by an arbitrary contract.
- **Tenant isolation:** a relationship under tenant X must not block a project under tenant Y.
- **Detection matrix:** cover every relationship type × every check context, with the expected block or allow from the rules table above.
- **Symmetry:** (A,B) and (B,A) produce the same `rel_id` and a single record.
- **Expiry:** test at `valid_until - 1`, `valid_until`, and `valid_until + 1`, and check that a revoked relationship is ignored by the gates.
- **Replay and idempotency:** a duplicate declare returns the same id with one event, and a double revoke emits only one event.
- **Privacy:** assert that no event payload contains `rel_type` or `evidence_hash`.
- **Bounds and budget:** a full 32×8 team check must stay within a recorded CPU and memory ceiling. The 33rd relationship for a party fails.
- **Errors:** pin the discriminants.
- **Upgrade:** decode a fixture `Relationship` after a WASM upgrade.

## Dependencies

`capstone-identifiers` (project → tenant). It is called by CAPSTONE-043 `capstone-team-acceptance` before activation, and by the review-assignment flow (future CAPSTONE review items) before a reviewer is bound with CAPSTONE-044 `Reviewer` scope. Conceptually it parallels `mentorship-conflicts`. The two do not share storage: mentorship relationships do not carry over into capstone checks automatically.

## Rollout and rollback

**Rollout:**
1. Deploy and initialize.
2. Have each tenant authority register its attesters.
3. Backfill attested relationships from the tenant's existing conflict-of-interest register, using evidence hashes only.
4. Turn on the gate in CAPSTONE-043.

**Rollback:** if the gate starts blocking legitimate activations, the CAPSTONE-043 admin can switch off its gate-enforcement flag. That action emits an event and must be time-boxed and recorded off-chain. The registry's data stays intact. Never bulk-revoke relationships as a rollback measure, because that destroys the audit trail.

## Corresponding CAPSTONE-045 work in other repositories

- **Backend:** the disclosure intake form, evidence storage with salted hashing, attester tooling and HR, sponsor, and payroll data checks, review-assignment service integration, and an indexer projection.
- **Frontend:** a self-declaration UI with a privacy notice, a "blocked by a conflict" message that does not reveal the conflict's type to third parties, and an attester console.
- **Policy:** the tenant-level conflict-of-interest policy, the appeal process, and a retention schedule for the off-chain evidence.
