# scholarship-access-control

Contract: `scholarship-access-control`  
Version: 1  
Audited against: `Gen-x-academy/chainVerse-onchain` `origin/main` commit `82c681a1fbdf5984b4b6adaee50c053af21bbe09`

---

## Purpose

Enforces the seven actor roles — Student, Sponsor, Reviewer, Verifier, Finance,
Auditor, and Administrator — on every scholarship resource, with program-scoped
bindings so cross-tenant access fails structurally rather than by policy.

---

## Roles

| Role | Scope | Typical holder |
|---|---|---|
| Student | Per-program | Award applicant |
| Sponsor | Per-program | Funding organisation |
| Reviewer | Per-program | Application evaluator |
| Verifier | Per-program | Identity/enrollment provider |
| Finance | Per-program | Payout executor |
| Auditor | Global (`[0x00;32]`) | Compliance/audit team |
| Administrator | Global | Contract deployer / ops |

Global roles (Auditor, Administrator) are stored under the all-zero scope and
are visible for every `has_role` / `authorize` call regardless of the
`program_id` argument.

---

## Permission table

The default table is seeded by `initialize` and can be overridden at any time
with `set_permission(admin, resource, role, operation, allowed)`.

The seven operations are: Read, Write, Approve, Reject, Disburse, Audit, Revoke.  
The eight resources are: Program, Application, Document, Decision, Disbursement, Report, Identity, Communication.

Defaults follow least-privilege (e.g. Students cannot Disburse; Reviewers cannot
Disburse; only Finance and Administrator hold Disburse on Disbursement).

---

## Storage

| Key | Storage tier | TTL |
|---|---|---|
| `Admin` | Instance | Contract lifetime |
| `Permission(resource, role, op)` | Instance | Contract lifetime |
| `TotalGrantCount` | Instance | Contract lifetime |
| `Role(account, program_id, role)` | Persistent | ~1–2 years |
| `ProgramGrantCount(program_id)` | Persistent | ~1–2 years |

Revoked grants are tombstoned to `false` rather than deleted, so the event
log remains the authoritative audit trail.

---

## Bounds

- `MAX_GRANTS_PER_PROGRAM = 256` — prevents per-program index bloat.
- `MAX_TOTAL_GRANTS = 50 000` — guards the instance counter from overflow.
- Both checked with arithmetic overflow protection.

---

## Events

| Symbol | Payload | When |
|---|---|---|
| `ROLEGRT` | `(account, program_id, role)` | Role granted |
| `ROLERVK` | `(account, program_id, role)` | Role revoked |
| `PERMSET` | `(resource, role, op, allowed)` | Permission table updated |

---

## Privacy

Only `Address` (Stellar public key) and `BytesN<32>` program identifiers are
stored. No names, grades, or PII are ever written. Revocation is immediate.

---

## Ownership

The contract admin (set at `initialize`) is the sole authority for grant/revoke
and permission changes. Admin transfer is not implemented in v1; a two-step
transfer following the certificate contract pattern is the recommended upgrade path.

---

## Migration

Adding a new `Role` or `Resource` variant does not affect existing storage keys.
Removing a variant requires a migration script that revokes outstanding grants
for that variant before the upgrade is applied.

---

## Operational impact

- `grant_role` / `revoke_role` are low-cost persistent writes (~1–2 ledger ops).
- `authorize` is two persistent reads + two instance reads; suitable for
  on-chain callers in hot paths.
- Permission table lives in instance storage: O(1) reads, no TTL expiry.
