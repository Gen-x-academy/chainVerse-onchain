//! Cross-contract test harness for the Scholarships On-chain epic.
//!
//! The five scholarship contracts are independently deployable and — per
//! ADR 0002 (`docs/adr/0002-scholarships-on-chain-boundaries.md`) — do not
//! call each other. `scholarship-applications` accepts submissions for a
//! `program_id` that `scholarship-core` may already consider `Archived`, and
//! nothing on-chain stops a reservation in `scholarship-programs` for a
//! program nobody ever published.
//!
//! That gap is a deliberate ADR decision (cross-contract enforcement is
//! listed as follow-up work), not something these tests can fix. What these
//! tests *can* do is pin the behaviour a real off-chain orchestrator depends
//! on, so that the orchestrator's checks are load-bearing rather than
//! decorative:
//!
//! - [`tests/journeys.rs`] (#1148) — full student/sponsor/reviewer/finance/
//!   administrator journeys, draft through final payment, plus role isolation,
//!   accessibility-critical actions, and failure recovery.
//! - [`tests/properties.rs`] (#1149) — generated action sequences checked
//!   against lifecycle, budget, and consent invariants.
//! - [`tests/load.rs`] (#1150) — burst submissions, reviews, and award
//!   batches at a deadline, plus tenant-isolation checks.
//! - [`tests/security.rs`] (#1151) — object authorization, wallet
//!   substitution, replay, retry, and stored-content abuse cases.
//! - [`tests/observability.rs`] (#1147) — the event stream, what a refused
//!   call announces, what the stream discloses about an applicant, and TTL
//!   liveness. The suites above all assert on state; none of them look at the
//!   fourteen events the contracts publish, so before this file existed a
//!   contract could emit a wrong topic, a wrong payload, or an event for a
//!   rejected operation and the suite would still have been green.
//!
//! [`World`] wires all five contracts onto one simulated ledger with a
//! named cast of actors. Every test builds its own `World`, so no test can
//! observe another's state.

pub mod fixture;
