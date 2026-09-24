# ADR 0002 — Scholarships On-Chain: Platform Boundaries and Invariants

- Status: Draft
- Deciders: Scholarships On-chain working group
- Date: 2026-09-24

## Context

The Scholarships On-chain epic has, over several passes, produced four
independent Soroban contracts without an up-front architecture record:

- `contracts/scholarship-core` — the `Program` record itself (stable ID,
  sponsor reference, title/description/currency/funding model) and its
  Draft/Published/Paused/Closed/Archived lifecycle.
- `contracts/scholarship-programs` — a program's application window
  (open/close/grace) and award inventory/budget ceiling.
- `contracts/scholarship-eligibility` — composable eligibility rules and
  expiring, revocable attestations.
- `contracts/scholarship-applications` — applicant submission, duplicate
  prevention, versioned form schemas, and versioned consent.

This ADR records the actor model, state machines, invariants, failure
modes, privacy boundaries, and explicit non-goals that those four
contracts already implicitly follow, and that any further scholarship
contract (sponsor organizations, team membership, withdrawal, receipts,
prerequisite/exclusion rules, disbursement) should be reviewed against.
It also separates this domain from the pre-existing, unrelated
"financial-aid CRUD" surface (if/when one exists elsewhere in this
repository) — scholarships, sponsorships, bursaries, awards, and
milestone-disbursements are their own trust boundary, not an extension of
general financial-aid record-keeping.

## Actors

| Actor | Represented by | Can do |
|---|---|---|
| **Applicant / Student** | `Address` (self-signed) | Record/revoke their own consent; submit an application for themselves only; never acts on another applicant's behalf. |
| **Issuer** | `Address`, admin-allowlisted in `scholarship-eligibility` | Issue and revoke attestations they themselves issued. |
| **Program Owner** | `Address`, set at `create_program` in `scholarship-core` | Transition their own program's lifecycle; (today) also the implicit authority `scholarship-programs`/`scholarship-eligibility` assume for window/budget/rule configuration per program, pending real sponsor-org roles. |
| **Sponsor** (organization) | *Not yet implemented* (#1059) | Will own one or more programs and delegate scoped roles to members (#1060) — reviewer, finance, program manager — instead of a single fixed `owner` address. |
| **Reviewer / Finance / Administrator** | *Not yet implemented* (#1059/#1060) | Named in #1057's acceptance criteria as role-scoped entry points; today, every contract in this epic only distinguishes "the admin who called `initialize`" and "everyone else," which is the gap #1059/#1060 and the registry in this pass exist to close. |

Every state-changing call in every contract in this epic requires
`require_auth()` from the actor it claims to act as — there is no
contract in this epic where a caller can mutate state on another
address's behalf implicitly. Where a broader "administrator" role is
needed, it is a single stored `Address` compared by value, never inferred.

## State machines

- **Program lifecycle** (`scholarship-core::ProgramStatus`): `Draft →
  Published → {Paused ⇄ Published, Closed} → Archived`, plus `Draft →
  Archived` directly. `Archived` is terminal — no contract in this epic
  allows a transition out of it. This is the *only* state machine with
  more than two states in the epic so far; every other contract models
  state as either presence/absence of a record (e.g. "has an
  application") or a small closed boolean-ish condition (active/inactive,
  revoked/not, open/closed-by-time).
- **Attestation validity** (`scholarship-eligibility`): a derived,
  two-input state — `valid = !revoked && expiry > now` — deliberately
  *not* modeled as an explicit enum, since it is fully determined by two
  stored fields plus ledger time and needs no separate transition
  function to "expire" it.
- **Application status** (`scholarship-applications`): currently a
  single terminal state, `Submitted` — withdrawal (#1076, not yet
  implemented) will add a second, and per the existing design must not
  delete or overwrite the submission record when it does (see
  Invariants, I5).

## Invariants

- **I1 — Exact authorization.** Every mutation requires `require_auth()`
  from the specific address whose authority it depends on (the applicant
  themselves, the specific issuer, the specific program owner, or the
  stored admin) — never a broader role standing in for a narrower one.
- **I2 — Checked arithmetic.** Every increment/decrement of a counter or
  balance (`scholarship-programs`' `awarded_count`/`committed_amount`,
  every version counter across the epic) uses `checked_add`/`checked_sub`
  and a typed error on overflow — never a bare `+`/`-` on-chain.
- **I3 — Bounded storage.** No contract in this epic stores an
  unboundedly-growing list on-chain. Versioned records (form schemas,
  consent terms, eligibility rules, program lifecycle) keep only the
  *latest* pointer plus each individual immutable version by key — never
  a growing `Vec` of history. Audit trails that need full history use
  events, not on-chain lists (see Privacy boundaries).
- **I4 — Immutable identifiers.** A `program_id`, once used in
  `scholarship-core::create_program`, is permanent — there is no rename
  or re-key operation anywhere in the epic. The same applies to a
  published form-schema/consent-terms/eligibility-rule version once
  written: `publish_*` always creates a new version, never mutates an
  existing one.
- **I5 — No silent rewrites of applicant-facing history.** An
  `Application` record, once submitted, is never overwritten by a later
  action. Consent can be revoked, but revocation does not retroactively
  alter a submission already made under it (`scholarship-applications`'
  `revoke_consent` doc comment is explicit about this). Withdrawal
  (#1076, not yet implemented) must follow the same rule: it should add a
  new status, not delete the record.
- **I6 — No overselling.** `scholarship-programs::reserve_award` can never
  push `awarded_count` past `max_recipients` or `committed_amount` past
  `total_budget`; both are enforced by a single atomic read-check-write
  per call, which Soroban's per-invocation serialization makes safe under
  concurrent callers without any additional locking primitive.
- **I7 — Deterministic evaluation.** Any function that answers "is X
  currently true" (`is_open`, `is_within_grace`, `has_valid_attestation`,
  `has_valid_consent`, `evaluate_eligibility`) is a pure function of
  on-chain state and `env.ledger().timestamp()` — never of caller-supplied
  "current time," randomness, or off-chain input. Two calls with
  unchanged state always agree.

## Failure modes

- **Stale consent after a republished terms bundle.** Publishing a new
  consent-terms version immediately invalidates every existing consent
  for that program (by design — I7/#1073's "changes trigger appropriate
  re-consent"). Operationally, this can strand in-progress applicants
  mid-intake-window with no on-chain signal beyond the `CONSPUB` event;
  any UI built on this epic must listen for that event and prompt
  affected applicants to re-consent, or terms changes should be avoided
  during an active window.
- **Cross-contract inconsistency.** None of the four contracts currently
  validate against each other — `scholarship-applications.submit_application`
  does not check `scholarship-core`'s program status or
  `scholarship-programs`' window/budget, and `scholarship-eligibility`'s
  rules are not enforced at submission time at all. A program could be
  `Archived` in `scholarship-core` while `scholarship-applications` still
  accepts submissions against its `program_id`. This is the primary
  motivation for the module registry introduced in this pass (see below)
  — a documented, on-chain-discoverable map of which contract owns which
  concern is the prerequisite for wiring them together safely, which
  remains a follow-up.
- **Issuer removal doesn't cascade.** Removing an issuer from
  `scholarship-eligibility`'s allowlist blocks new attestations but does
  not revoke ones they already issued; a compromised-issuer incident
  requires explicitly revoking each of their attestations.
- **Budget reconfiguration after reservations.** `configure_award_budget`
  can currently be called again after awards have already been reserved,
  silently resetting counters. Documented as a known gap in
  `scholarship-programs`' own docs; a guard against this is a follow-up.

## Privacy boundaries

- **On-chain, always:** stable identifiers (program/applicant addresses,
  program IDs), status/lifecycle enums, timestamps, version numbers,
  aggregate counters (award counts, committed amounts), and integrity
  commitments (`BytesN<32>` hashes) over off-chain content.
- **Off-chain, always:** the actual content any commitment hashes over —
  application answers, documents, form schema definitions, consent-terms
  text, and the evidence backing any eligibility attestation (grades,
  income figures, enrollment records). No contract in this epic has a
  code path that accepts or stores this content directly.
- **Never derived on-chain from PII:** eligibility evaluation
  (`evaluate_eligibility`) only ever consults attestation *existence and
  validity*, never the claim's underlying evidence — the contract cannot
  answer "why" a subject holds an attestation, only "does a valid one
  exist."

## Non-goals (explicitly out of scope for the on-chain contracts)

- **Identity verification / KYC.** No contract in this epic verifies who
  an `Address` belongs to; attestation issuance trust comes entirely from
  the admin-managed issuer allowlist, not from any on-chain identity
  proof.
- **Document storage, scanning, or encryption.** Secure document upload
  (#1072) is explicitly an off-chain-heavy concern; this epic's contracts
  only ever reference off-chain content by hash.
- **Rich answer validation** (word limits, normalized rich text, #1071) —
  client/server-side off-chain validation, not on-chain enforcement.
- **Disbursement / payment execution.** Award budget *accounting*
  (`scholarship-programs`) is on-chain; actually moving funds to a
  recipient is not implemented by any contract in this epic yet, and when
  it is, it should be reviewed as its own ADR extension given the payment
  contracts already in this repository (`payment`, `payout-automation`,
  `escrow`, `escrow-vault`).
- **Cross-contract enforcement** (a program's `Archived` status blocking
  submissions, a budget's exhaustion blocking eligibility, etc.) — see
  Failure modes; this is real, scoped follow-up work, not a rejected
  concern.

## ABI / Interface impact

- All four contracts use `#![no_std]` with `#[contract]`/`#[contractimpl]`/
  `#[contracttype]`/`#[contracterror]`, matching the rest of this
  workspace (e.g. `course_registry`, `escrow-vault`).
- Each contract has its own `Result<_, ContractError>` enum with unique,
  sequential discriminants — no shared error type across contracts (they
  are independently deployable).
- This pass adds `contracts/scholarship-registry` (see below), whose ABI
  is intentionally small: register/resolve a module address by name, and
  grant/revoke/check a coarse role per address. It does not re-export or
  wrap the other four contracts' interfaces.

## Storage impact

- Every contract in this epic uses `persistent` storage for records with
  an explicit `extend_ttl` bump (~1 year at 6-second ledgers, matching
  `course_registry`'s convention) and `instance` storage only for the
  single `Admin` address.
- No contract stores a growing collection (see I3); every "history" need
  is served by immutable, individually-keyed versions plus events.

## Event impact

- Every mutation across the epic publishes an event with a
  `symbol_short!` topic (`PROGNEW`, `PROGTRAN`, `SUBMIT`, `RULEPUB`,
  `ATTEST`, `WINSET`, `BUDGSET`, `AWDRSV`, `FORMPUB`, `CONSPUB`,
  `CONSENT`), so an off-chain indexer can reconstruct full history
  (including transition sequences that aren't kept on-chain past their
  latest value — see I3) uniformly across all four contracts.

## Deployment & migration impact

- Each contract is deployed independently; none references another's
  contract address today (see Failure modes — cross-contract
  inconsistency). Wiring them together (e.g.
  `scholarship-applications` checking `scholarship-core`'s program
  status) is future work, and the registry this pass adds is the
  discovery mechanism that future work will use.
- No existing, non-scholarship contract's storage layout is touched by
  any contract in this epic.

## Consequences

- **Positive:** a documented, reviewable boundary for a domain that had
  been growing without one; the module registry gives future
  cross-contract wiring a single place to resolve "which contract handles
  X" instead of hardcoding addresses; the coarse role registry gives
  #1057's "role-scoped routes... unauthorized actors receive consistent
  errors" a real, testable mechanism ahead of full sponsor-org roles
  (#1059/#1060).
- **Negative:** this ADR is written after four contracts already exist,
  so it documents boundaries partly by observation rather than purely by
  up-front design — any place where an existing contract's behavior
  conflicts with an invariant stated here should be treated as a bug in
  that contract, filed as a follow-up, not as license to reinterpret the
  invariant.
