# scholarship-aggregate-analytics

Soroban smart contract for privacy-safe aggregate analysis of scholarship
eligibility, completion, review, decision, and award outcomes.

Audited against **Gen-x-academy/chainVerse-onchain origin/main commit 82c681a1**.

---

## Purpose

Scholarship programmes need outcome metrics (approval rates, disbursement
totals, funnel drop-offs) to detect systemic bias, allocate resources, and
report to funders — but those metrics must never expose individual applicants.
This contract accumulates **only** counts and totals, enforces a configurable
k-anonymity suppression threshold on every read, and provides per-metric
definitions and caveats so consumers understand exactly what each number means.

---

## Ownership

| Role | Stored as | Responsibilities |
|---|---|---|
| `admin` | instance storage | Governance: pause, update k-threshold, upsert metric definitions, rotate recorder, upgrade |
| `recorder` | instance storage | The only address authorised to push metric updates (counts, disbursements). Should be a backend service account, not an EOA. |

---

## Privacy design

### k-Anonymity suppression

The contract stores a configurable `k_threshold` (minimum 5, enforced on-chain).
`get_cohort_metrics` returns `CohortResult::Suppressed` whenever
`total_applicants < k_threshold`. The suppression response includes the current
threshold and a plain-language reason, but **zero** underlying metric values.

The admin may raise the threshold at any time — this immediately suppresses
previously visible cohorts that no longer meet the new threshold.

### No individual identifiers

No applicant addresses, application IDs, wallet addresses, or any linkable
identifier may be stored in this contract. The `recorder` role is responsible
for aggregating data off-chain and submitting only counts.

### Metric definitions and caveats

Every metric exposed by the contract may be accompanied by a `MetricDefinition`
record (upsertable by admin) that describes what the number means, how it was
counted, and any applicable caveats (e.g. snapshot date, exclusion criteria).
Consumers of the API should always check for and display these definitions.

---

## Entry points

### Lifecycle

| Function | Caller | Description |
|---|---|---|
| `init(admin, recorder, k_threshold)` | `admin` | One-time init. `k_threshold` is floored to 5 if a lower value is supplied. |
| `upgrade(caller, new_wasm_hash)` | `admin` | WASM upgrade. |

### Metric recording

| Function | Caller | Description |
|---|---|---|
| `create_cohort(caller, cohort_tag)` | `recorder` | Create a new cohort bucket. Idempotent if cohort already exists. |
| `increment(caller, cohort_tag, field, delta)` | `recorder` | Increment a counter field by `delta`. Checked arithmetic — returns `ArithmeticOverflow` rather than wrapping. |
| `decrement(caller, cohort_tag, field, delta)` | `recorder` | Correct an over-count. Returns `CounterUnderflow` if result would go below zero. |
| `record_disbursements(caller, batch)` | `recorder` | Atomically add a batch of disbursement amounts. Accumulates `disbursed_count` and `total_disbursed_amount`. |

### Queries

| Function | Returns |
|---|---|
| `get_cohort_metrics(cohort_tag)` | `CohortResult::Metrics(CohortMetrics)` or `CohortResult::Suppressed(SuppressedResult)` |
| `k_threshold()` | `u64` |
| `get_metric_definition(key)` | `Option<MetricDefinition>` |

### Admin operations

| Function | Caller |
|---|---|
| `pause(caller)` | `admin` |
| `unpause(caller)` | `admin` |
| `set_k_threshold(caller, new_k)` | `admin` — floored to 5 |
| `set_recorder(caller, new_recorder)` | `admin` |
| `upsert_metric_definition(caller, key, name, definition)` | `admin` |

---

## Metric fields

`MetricField` is a closed enum — callers cannot supply arbitrary string keys,
preventing storage griefing.

| Variant | Description |
|---|---|
| `TotalApplicants` | All applications ingested into this cohort |
| `EligibleCount` | Applications that passed static eligibility checks |
| `ReviewCompletedCount` | Applications with a completed review |
| `ApprovedCount` | Applications receiving a positive decision |
| `RejectedCount` | Applications receiving a negative decision |
| `WithdrawnCount` | Withdrawn or incomplete applications |
| `DisbursedCount` | Awards actually paid out |

`total_disbursed_amount` (i128) is updated only via `record_disbursements`.

---

## CohortResult variants

```
CohortResult::Metrics(CohortMetrics)
  — returned when total_applicants >= k_threshold
  — contains all counter values and timestamps

CohortResult::Suppressed(SuppressedResult)
  — returned when total_applicants < k_threshold
  — contains only: cohort_tag, k_threshold, reason string
  — zero underlying data exposed
```

Callers **must** match on the variant before using data. Treating
`Suppressed` as `Metrics` is a client-side error.

---

## Storage layout

| Key | Class | TTL | Description |
|---|---|---|---|
| `DataKey::Admin` | instance | contract instance TTL | Admin address |
| `DataKey::Paused` | instance | contract instance TTL | Pause flag |
| `DataKey::Recorder` | instance | contract instance TTL | Recorder address |
| `DataKey::KThreshold` | instance | contract instance TTL | k-anonymity threshold |
| `DataKey::CohortMetrics(String)` | persistent | ~1–2 years | Per-cohort aggregate counters |
| `DataKey::MetricDefinition(String)` | persistent | ~1–2 years | Per-metric definition and caveat |

---

## Events

| Topic | Data | Emitted by |
|---|---|---|
| `AA_INIT` | `(admin, recorder, k_threshold)` | `init` |
| `AA_CRTC` | `(cohort_tag, timestamp)` | `create_cohort` |
| `AA_INCR` | `(cohort_tag, field, delta)` | `increment` |
| `AA_DECR` | `(cohort_tag, field, delta)` | `decrement` |
| `AA_DISB` | `(cohort_tag, count, new_total)` | `record_disbursements` |
| `AA_MDEF` | `(key, caller)` | `upsert_metric_definition` |
| `AA_SETK` | `(caller, new_k)` | `set_k_threshold` |
| `AA_SREC` | `(caller, new_recorder)` | `set_recorder` |
| `AA_PAUS` | `caller` | `pause` |
| `AA_UPAU` | `caller` | `unpause` |
| `AA_UPGD` | `new_wasm_hash` | `upgrade` |

---

## Off-chain trust boundary

The contract is a **coordination layer** — it does not verify that the counts
pushed by the `recorder` are accurate. The off-chain recorder must:

1. Aggregate application pipeline events from authoritative sources.
2. Submit only counts (never raw applicant data).
3. Use `decrement` to correct errors immediately — with an audit log entry.
4. Never submit counts for cohorts smaller than the configured `k_threshold`
   without understanding that the cohort will be suppressed.

The `recorder` key should be rotated on a schedule and should never be an
externally-owned account.

---

## Migration

When cohort definitions change (e.g. a new cohort segmentation scheme):

1. Create new cohort tags with `create_cohort`.
2. Old cohort data remains in storage and is still queryable.
3. If metric fields change semantics, update the `MetricDefinition` records
   immediately before deploying the new recorder logic.

**Breaking ABI changes** must be listed in `.github/scholarship-compat-override.json`.

---

## Operational impact

- **Pause:** Blocks `create_cohort`, `increment`, `decrement`,
  `record_disbursements`. Queries (`get_cohort_metrics`, `k_threshold`,
  `get_metric_definition`) still work.
- **k-threshold change:** Takes effect immediately on the next query.
  Existing cohort data is not modified.
- **Recorder rotation:** New recorder takes effect immediately. In-flight
  requests from the old recorder will fail auth after rotation.

---

## Security properties

- Closed `MetricField` enum prevents storage key injection.
- All counter arithmetic uses `checked_add` / `checked_sub` — overflow and
  underflow are returned as typed errors, never silently wrapped.
- `k_threshold` has a hard floor of 5 — cannot be set to 1 or 0 to bypass
  suppression.
- `require_auth()` called before any storage access on all mutating calls.
- Cohort tag bounded to 64 bytes; metric key to 32 bytes; definition to 512 bytes.
