# scholarship-fairness-review

Soroban smart contract that manages versioned scholarship eligibility and scoring
rule-sets, gates every change through a privacy and fairness review, and enforces
a high-risk second-approver workflow before any rule change is activated.

Audited against **Gen-x-academy/chainVerse-onchain origin/main commit 82c681a1**.

---

## Purpose

Scholarship rule changes (scoring weights, eligibility thresholds, document
requirements) carry material risk of encoding prohibited proxies or producing
disparate impact on protected groups. This contract provides an immutable,
on-chain audit trail of every rule version, its reviewer, its outcome, and the
rollback path to any prior approved version.

---

## Ownership

| Role | Stored as | Responsibilities |
|---|---|---|
| `admin` | instance storage | Propose rule versions, activate/rollback, pause contract, two-step admin transfer |
| `reviewer` | instance storage | Record privacy and fairness review outcomes (Approved / Rejected / ApprovedWithConditions) |
| `high_risk_approver` | instance storage | Second sign-off required for `RiskLevel::High` changes before activation |

### Separation of duties

The `admin`, `reviewer`, and `high_risk_approver` should be **distinct** addresses
controlled by distinct key-holders.  The `high_risk_approver` must differ from the
proposer (version owner) at activation time — enforced on-chain.

---

## Entry points

### Lifecycle

| Function | Caller | Description |
|---|---|---|
| `init(admin, reviewer, high_risk_approver)` | `admin` | One-time initialisation. Idempotent guard: rejects if already initialised. |
| `upgrade(caller, new_wasm_hash)` | `admin` | WASM upgrade. Admin-gated. |

### Rule version management

| Function | Caller | Description |
|---|---|---|
| `propose_rule_version(caller, risk_level, rule_payload, change_summary, test_cohort_tag, replaces_version)` | `admin` | Create a new pending version. Returns the assigned version number. |
| `record_review(caller, version, outcome, notes)` | `reviewer` | Record a review outcome. Can only be called once per version (no overwrite). |
| `second_approve(caller, version)` | `high_risk_approver` | Required second sign-off for `RiskLevel::High` versions. Approver ≠ proposer enforced. |
| `activate_version(caller, version)` | `admin` | Make a reviewed-and-approved version the live rule-set. |
| `rollback_to(caller, target_version)` | `admin` | Revert to any prior approved version. Emits `FR_RLLB`. |

### Queries

| Function | Returns |
|---|---|
| `get_version_summary(version)` | `RuleVersionSummary` (no reviewer notes) |
| `active_version()` | `u64` — currently live version (0 = none) |
| `next_version_preview()` | `u64` — version number that the next proposal would receive |
| `paused()` | `bool` |

### Admin operations

| Function | Caller |
|---|---|
| `pause(caller)` | `admin` |
| `unpause(caller)` | `admin` |
| `set_reviewer(caller, new_reviewer)` | `admin` |
| `set_high_risk_approver(caller, new_approver)` | `admin` |
| `propose_admin(caller, new_admin)` | `admin` — starts 30-day handoff |
| `accept_admin(caller)` | `new_admin` — completes handoff |

---

## Storage layout

| Key | Class | TTL | Description |
|---|---|---|---|
| `DataKey::Admin` | instance | contract instance TTL | Admin address |
| `DataKey::Paused` | instance | contract instance TTL | Pause flag |
| `DataKey::Reviewer` | instance | contract instance TTL | Reviewer address |
| `DataKey::HighRiskApprover` | instance | contract instance TTL | Second-approver address |
| `DataKey::PendingAdmin` | instance | contract instance TTL | Pending admin during handoff |
| `DataKey::PendingAdminExpiry` | instance | contract instance TTL | Expiry timestamp for pending admin |
| `DataKey::RuleVersion(u64)` | persistent | MIN 3 110 400 / MAX 6 220 800 ledgers (~1–2 years) | Full `RuleVersion` struct per version |
| `DataKey::NextVersion` | persistent | same | Monotonic version counter |
| `DataKey::ActiveVersion` | persistent | same | Currently active version number |

Instance storage TTL is bumped on every entry-point call.  Persistent keys are
bumped on every read and write.

---

## Events

All topics are `symbol_short!` values (≤ 9 chars).

| Topic | Data | Emitted by |
|---|---|---|
| `FR_INIT` | `(admin, reviewer, high_risk_approver)` | `init` |
| `FR_PROP` | `(version, caller, risk_level, cohort_tag, replaces_version, timestamp)` | `propose_rule_version` |
| `FR_RVWD` | `(version, caller, outcome, timestamp)` | `record_review` |
| `FR_2APP` | `(version, caller, timestamp)` | `second_approve` |
| `FR_ACTV` | `(version, caller, prev_active_version, timestamp)` | `activate_version` |
| `FR_RLLB` | `(target_version, caller, prev_active_version, timestamp)` | `rollback_to` |
| `FR_PADM` | `(caller, new_admin, expiry)` | `propose_admin` |
| `FR_AADM` | `caller` | `accept_admin` |
| `FR_PAUS` | `caller` | `pause` |
| `FR_UPAU` | `caller` | `unpause` |
| `FR_SRVW` | `(caller, new_reviewer)` | `set_reviewer` |
| `FR_SHKA` | `(caller, new_approver)` | `set_high_risk_approver` |
| `FR_UPGD` | `new_wasm_hash` | `upgrade` |

---

## Privacy design

1. **No PII on-chain.** `rule_payload` must be a content-addressed identifier
   (IPFS CID, SHA-256 hex) pointing to an off-chain document. Never embed
   applicant names, addresses, or government IDs in any field.

2. **No protected-attribute data.** The reviewer is expected to evaluate the
   off-chain rule document for prohibited proxies (geography, name patterns,
   institutional tier lists) before recording `Approved`. The `notes` field
   should reference the off-chain analysis report.

3. **`RuleVersionSummary` omits reviewer notes.** The public query
   `get_version_summary` does not expose internal deliberation captured in
   `reviewer_notes`. Full records are accessible only to parties who can
   query persistent storage directly.

4. **High-risk gate.** Changes classified as `RiskLevel::High` (e.g. scoring
   weight changes, threshold reductions, new eligibility criteria touching
   demographics) require both a reviewer approval and an independent second
   approver. This prevents a single actor from pushing a discriminatory rule
   change unilaterally.

---

## Rollback path

Every activated version stays in persistent storage indefinitely (subject to TTL
refresh). To roll back:

1. Call `rollback_to(admin, target_version)`.
2. The target must have `review_outcome = Approved | ApprovedWithConditions`.
3. The previously active version is deactivated in the same call.
4. A `FR_RLLB` event is emitted with the old and new version numbers for audit.

There is no concept of "deleting" a version. The full history is always
queryable.

---

## High-risk change workflow

```
admin.propose_rule_version(..., risk_level=High, ...)
  → version N created, status=Pending

reviewer.record_review(N, Approved, notes)
  → version N, status=Approved

high_risk_approver.second_approve(N)          ← must differ from admin
  → version N, second_approver set

admin.activate_version(N)
  → version N becomes active, previous version deactivated
```

Attempting `activate_version` before `second_approve` returns
`HighRiskApprovalRequired`.

---

## Migration

This contract has no dependency on other scholarship contracts. It is a
standalone audit log. If the rule-set format changes (e.g. moving from JSON
to protobuf):

1. Propose a new version with the new format's CID as `rule_payload`.
2. Update the off-chain rule engine to read the new format.
3. Record a review and activate the new version.
4. The old versions remain in storage as rollback targets.

**Breaking ABI changes** (entry-point renames, removed parameters) must be
listed in `.github/scholarship-compat-override.json` with a `migration` note
before merging, to pass the `scholarship_surface.py --check` CI gate.

---

## Operational impact

- **Pause:** All state-mutating calls (`propose_rule_version`, `record_review`,
  `second_approve`, `activate_version`, `rollback_to`) are blocked.
  Admin operations (`pause`, `unpause`, `set_reviewer`, `propose_admin`) still work.
  Read-only queries (`get_version_summary`, `active_version`) still work.
- **Pending admin expiry:** 30 days. If the new admin does not call
  `accept_admin` within that window, the proposal is silently voided and
  a new one must be issued.
- **TTL:** Persistent records have a ~1-year minimum TTL refreshed on every
  access. Records not accessed for 2 years may expire and require manual
  TTL extension via a dedicated helper or contract upgrade.

---

## Security properties

- `require_auth()` is called on every mutating entry point before any storage read.
- All arithmetic uses `checked_add` / `saturating_add` — no overflow possible.
- `rule_payload` is bounded to 4 096 bytes; `test_cohort_tag` to 64 bytes.
- The init guard checks for existing admin before writing (double-init safe).
- Two-step admin transfer with 30-day expiry prevents admin key lock-out.
