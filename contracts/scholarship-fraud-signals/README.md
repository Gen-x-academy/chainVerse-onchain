# scholarship-fraud-signals

Soroban smart contract for flagging likely duplicate applicants, reused
documents, manipulated evidence, and coordinated submissions in the scholarship
pipeline — with explainable signals, human review gates, and a full appeal path.

Audited against **Gen-x-academy/chainVerse-onchain origin/main commit 82c681a1**.

---

## Purpose

Automated integrity checks on scholarship applications reduce manual workload
and surface patterns invisible to individual reviewers. However, automated
signals carry real harm risk if they trigger adverse actions without human
oversight. This contract:

- Stores fraud signals as **advisory flags only** — they carry no enforcement power.
- Requires a human reviewer to act before any signal reaches a terminal state.
- Provides a full appeal path so applicants can challenge incorrect signals.
- Prevents storage of biometric or sensitive personal data on-chain.
- Exposes explainable descriptions and evidence references for every signal.

---

## No-auto-reject guarantee

Raising a signal via `raise_signal` places the record in `SignalStatus::Flagged`.
**No downstream contract or off-chain process may automatically reject an
application** based solely on the presence of a flagged signal. The status must
reach `Confirmed` (human reviewer) or `AppealRejected` (after human review of
appeal) before any adverse action is taken. This is a **design invariant**, not
merely a policy suggestion.

---

## Ownership

| Role | Stored as | Responsibilities |
|---|---|---|
| `admin` | instance storage | Governance: pause, rotate detector/reviewer, file appeals, upgrade |
| `detector` | instance storage | Trusted service account that may raise and update signals. Should be automated backend infrastructure, not an EOA. |
| `reviewer` | instance storage | Human reviewer who resolves signals and responds to appeals. |

---

## Signal lifecycle

```
Detector raises signal
  → status: Flagged

  ┌─ Reviewer resolves ──────────────────────────────────────────┐
  │   resolve_signal(..., Confirmed, notes)  → Confirmed          │
  │   resolve_signal(..., Dismissed, notes)  → Dismissed          │
  └───────────────────────────────────────────────────────────────┘

  OR

  Admin files appeal
  → status: UnderAppeal

  ┌─ Reviewer responds to appeal ─────────────────────────────────┐
  │   resolve_signal(..., AppealUpheld, notes)   → AppealUpheld   │
  │   resolve_signal(..., AppealRejected, notes) → AppealRejected │
  └───────────────────────────────────────────────────────────────┘
```

`Confirmed`, `Dismissed`, `AppealUpheld`, and `AppealRejected` are **terminal
states** — a signal in one of these states cannot be resolved again.

---

## Entry points

### Lifecycle

| Function | Caller | Description |
|---|---|---|
| `init(admin, detector, reviewer)` | `admin` | One-time initialisation. |
| `upgrade(caller, new_wasm_hash)` | `admin` | WASM upgrade. |

### Signal management

| Function | Caller | Description |
|---|---|---|
| `raise_signal(caller, app_id, category, description, evidence_ref, risk_score)` | `detector` | Create a new signal in `Flagged` state. `app_id` must be unique per application. |
| `update_signal(caller, app_id, evidence_ref, risk_score)` | `detector` | Revise evidence reference and score of an existing signal. Does not change status. |
| `resolve_signal(caller, app_id, resolved_status, notes)` | `reviewer` | Record a human resolution. Signal must be `Flagged` or `UnderAppeal`. |
| `file_appeal(caller, app_id, appeal_statement)` | `admin` | File an appeal on behalf of an applicant. Signal transitions to `UnderAppeal`. |

### Queries

| Function | Returns |
|---|---|
| `get_signal_summary(app_id)` | `FraudSignalSummary` (no evidence_ref, no reviewer_notes) |
| `get_signal_full(app_id)` | `FraudSignal` (full record — gate off-chain to authorised reviewers) |
| `paused()` | `bool` |

### Admin operations

| Function | Caller |
|---|---|
| `pause(caller)` | `admin` |
| `unpause(caller)` | `admin` |
| `set_detector(caller, new_detector)` | `admin` |
| `set_reviewer(caller, new_reviewer)` | `admin` |

---

## Signal categories

| Variant | Trigger guidance |
|---|---|
| `LikelyDuplicate` | Same or near-identical submission content from distinct actors |
| `DocumentReuse` | Credential or document appears in multiple applications |
| `ManipulatedEvidence` | Inconsistent or implausible evidence values |
| `CoordinatedSubmission` | Timing, structure, or network patterns suggesting orchestrated bulk submission |
| `Other` | Catch-all; prefer a specific category where available |

Categories are coarse by design — they describe the nature of the concern
without exposing specifics about the detection method, which might help bad
actors evade detection.

---

## Risk score

`risk_score` is an integer in [0, 100].

- 0 means the signal is raised for monitoring or pattern-logging purposes
  with no current evidence of actual risk.
- 100 means the detector is highly confident the submission is fraudulent.
- The score is **advisory only**. No threshold on this value may trigger
  automatic rejection. Human review is mandatory.

---

## Privacy constraints

1. **No biometric data.** Fingerprint hashes, face embeddings, voice prints,
   or any biometric identifier must never appear in any field of a signal record.

2. **Evidence by reference, not value.** `evidence_ref` must be a
   content-addressed pointer (IPFS CID, SHA-256 hex) to an off-chain evidence
   bundle. The bundle itself may contain sensitive material, but that material
   lives off-chain under access control.

3. **No government IDs.** Passport numbers, national ID numbers, tax IDs, and
   similar sensitive identifiers must not appear in `app_id`, `description`,
   `evidence_ref`, or `appeal_statement`.

4. **Summary hides sensitive fields.** `get_signal_summary` returns
   `FraudSignalSummary`, which excludes `evidence_ref` and `reviewer_notes`.
   Full records must be gated to authorised reviewers in the off-chain layer.

5. **`app_id` must be opaque.** It should be a random UUID or a hash of the
   application identifier — not a wallet address or a name.

---

## Storage layout

| Key | Class | TTL | Description |
|---|---|---|---|
| `DataKey::Admin` | instance | contract instance TTL | Admin address |
| `DataKey::Paused` | instance | contract instance TTL | Pause flag |
| `DataKey::Detector` | instance | contract instance TTL | Detector address |
| `DataKey::Reviewer` | instance | contract instance TTL | Reviewer address |
| `DataKey::Signal(String)` | persistent | ~1–2 years | Full `FraudSignal` per application |

---

## Events

| Topic | Data | Emitted by |
|---|---|---|
| `FS_INIT` | `(admin, detector, reviewer)` | `init` |
| `FS_RAIS` | `(app_id, category, risk_score, caller, timestamp)` | `raise_signal` |
| `FS_UPDT` | `(app_id, new_risk_score, caller)` | `update_signal` |
| `FS_RESV` | `(app_id, resolved_status, caller, timestamp)` | `resolve_signal` |
| `FS_APPL` | `(app_id, caller, timestamp)` | `file_appeal` |
| `FS_PAUS` | `caller` | `pause` |
| `FS_UPAU` | `caller` | `unpause` |
| `FS_SDET` | `(caller, new_detector)` | `set_detector` |
| `FS_SRVW` | `(caller, new_reviewer)` | `set_reviewer` |
| `FS_UPGD` | `new_wasm_hash` | `upgrade` |

---

## Field size limits

| Field | Limit |
|---|---|
| `app_id` | 64 bytes |
| `description` | 512 bytes |
| `evidence_ref` | 256 bytes |
| `reviewer_notes` / `appeal_statement` | 1 024 bytes |
| `risk_score` | 0–100 (integer) |

---

## Migration

Signal records are keyed by `app_id`. If the application ID scheme changes:

1. Raise new signals under new IDs.
2. Old signals remain accessible under old IDs.
3. If the `FraudSignal` struct gains new fields, a contract upgrade is required.
   Existing records will have default values for new fields after upgrade.

**Breaking ABI changes** must be listed in `.github/scholarship-compat-override.json`.

---

## Operational impact

- **Pause:** Blocks `raise_signal`, `update_signal`, `resolve_signal`,
  `file_appeal`. Read-only queries still work.
- **Detector rotation:** New detector takes effect immediately. Signals
  already raised by the old detector remain valid.
- **Reviewer rotation:** New reviewer takes effect immediately. Pending
  signals awaiting review are not affected — any reviewer can resolve any
  signal.

---

## Security properties

- `require_auth()` called before any storage access on all mutating calls.
- `SignalStatus` transition rules enforced on-chain — only `Flagged` and
  `UnderAppeal` can be resolved; terminal states cannot be overwritten.
- `resolved_status` must be one of `{Confirmed, Dismissed, AppealUpheld,
  AppealRejected}` — other variants return `Unauthorized`.
- `risk_score > 100` returns `InvalidScore`.
- One appeal per signal enforced by `has_appeal` flag — `AppealAlreadyFiled`
  prevents duplicate appeals.
- All string fields bounded — no unbounded storage writes possible.
