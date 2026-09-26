# Scholarship Milestones Contract

- Status: Implemented (partial — see Scope)
- Owner: Scholarships On-chain working group
- Related issues: #1094 (submit milestone evidence), #1095 (verify
  milestone evidence)

## Scope

This contract (`contracts/scholarship-milestones`) implements two of the
"Scholarships On-chain/Milestones" epic's behaviors:

- **#1094 — Submit milestone evidence.** `submit_evidence()` records a
  caller-supplied `data_hash: BytesN<32>` — an integrity commitment over
  off-chain evidence — against a milestone that is currently `Active`.
  Evidence is keyed and **versioned** by `(program_id, milestone_id,
  recipient)`: a submission that differs from the current version creates
  version N+1, and re-submitting the *exact same* `data_hash` is
  idempotent — it returns the existing version and writes nothing, so a
  retried call can never create a duplicate. Authorization is exact: the
  caller must be the `recipient` themselves, or an admin-allowlisted
  trusted submitter acting on the recipient's behalf.
- **#1095 — Verify milestone evidence.** `verify_evidence()` lets an
  admin-allowlisted verifier decide one specific evidence version as
  `Approved`, `Rejected`, or `ChangesRequested`, each carrying an explicit
  `ReasonCode`. A verifier who is the evidence's `recipient` or
  `submitted_by` is rejected with `VerifierConflict`. Each version is
  decidable **at most once** (`AlreadyDecided` on any retry), which is what
  guarantees an approval emits at most one payment-eligibility event
  (`EVPASS`); every decision is stored on the version and emitted as
  `EVDECID`.

`ReasonCode` is a closed enum (`MeetsCriteria`, `InsufficientEvidence`,
`EvidenceMismatch`, `OutsideScope`) and must be coherent with the decision:
`Approved` requires `MeetsCriteria`; the other decisions require one of the
three negative codes. An incoherent pair is rejected with
`InvalidReasonCode` before any state changes.

**Not implemented here** (tracked separately / out of scope): actually
moving funds is the disbursements contract's job
(`scholarship-disbursements`, issues #1096/#1097) — this contract only
records the decision and the eligibility signal. There is no "assignment"
step distinct from the admin-managed verifier allowlist; a per-milestone
verifier roster is a natural follow-up if the flat allowlist proves too
coarse.

## Privacy

The contract never stores the evidence itself. On-chain state only ever
contains stable identifiers (addresses, `program_id`, `milestone_id`),
a `BytesN<32>` integrity commitment (`data_hash`) over off-chain content,
status/reason enums, and timestamps. Where evidence is sensitive, it is
encrypted off-chain before its hash is committed. `ReasonCode` is a bounded
enum rather than free text precisely so a verifier cannot leak sensitive
detail through a "reason" string; the real rationale stays off-chain. No
contract code path accepts or stores evidence content.

## Ownership

The admin (`initialize`) exclusively controls the milestone registry
(`create_milestone` / `set_milestone_status`) and both allowlists
(submitters and verifiers). A recipient can only submit evidence for
themselves; a trusted submitter can submit on a recipient's behalf only
while allowlisted. Only an allowlisted verifier can decide evidence, and
never evidence they submitted or that is their own. Removing a verifier
blocks *new* decisions from them; it does not retroactively change
decisions they already recorded — those remain part of the audit history.

## Migration

New contract, no prior on-chain state. Setup order: `initialize` →
`create_milestone` → `add_submitter`/`add_verifier` as needed. A milestone
must exist and be `Active` before `submit_evidence` succeeds
(`MilestoneNotFound` / `MilestoneNotActive` otherwise). There is no bulk
import path from an off-chain system in this pass.

## Operational impact

- **Decisions are terminal per version.** A changed decision (e.g.
  `ChangesRequested` → `Approved`) requires submitting a new evidence
  version; the old version and its decision are preserved. This is the
  mechanism behind "approval triggers at most one payment eligibility
  event" — the `EVPASS` event is emitted only on the transition of a
  `Pending` version to `Approved`, and a version can make that transition
  at most once.
- **Closing a milestone does not erase evidence.** `set_milestone_status`
  only gates *new* submissions; existing versions and decisions remain
  readable.
- **Reason codes are coarse by design.** An off-chain system that needs
  richer rationale should store it off-chain alongside the version; the
  enum on-chain is an auditable category, not the full explanation.
- **Storage/TTL/events** follow the sibling scholarship contracts
  (persistent storage, ~1-year TTL bump on write, `MSTONE`/`MSTAT`/
  `EVSUBMIT`/`EVDECID`/`EVPASS` events).
- **Cross-contract wiring** (e.g. `scholarship-programs` budget accounting
  reacting to `EVPASS`, or an indexer turning it into an intent in
  `scholarship-disbursements`) is a follow-up, consistent with ADR 0002's
  "cross-contract enforcement" non-goal.
