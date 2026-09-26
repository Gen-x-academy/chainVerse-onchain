# Scholarship guide

Role-based operating guide for the scholarship contracts: who does what, what
each role can actually call, what is visible on-chain, and what to do when
something fails.

This is the *narrative* guide. For exhaustive per-function reference, including
storage layout and error-by-error semantics, see the per-contract documents
listed under [Related documents](#related-documents). Where the two disagree,
the per-contract document is authoritative and this one is a bug.

## Contents

- [The seven contracts](#the-seven-contracts)
- [Roles and authorisation](#roles-and-authorisation)
- [Applicant guide](#applicant-guide)
- [Sponsor guide](#sponsor-guide)
- [Reviewer guide](#reviewer-guide)
- [Finance guide](#finance-guide)
- [Privacy](#privacy)
- [Known gaps](#known-gaps)
- [Troubleshooting](#troubleshooting)
- [Incident actions](#incident-actions)
- [Related documents](#related-documents)

## The seven contracts

| Contract | Responsibility |
| --- | --- |
| `scholarship-core` | Program records and their lifecycle |
| `scholarship-registry` | Which contracts are wired in, and who holds which role |
| `scholarship-programs` | Application windows and award budgets |
| `scholarship-eligibility` | Eligibility rules and attestations |
| `scholarship-applications` | Forms, consent, and application submission |
| `scholarship-milestones` | Milestone definition, evidence, and verification |
| `scholarship-disbursements` | Payout wallets, disbursement intents, execution |

These do not call each other. A program identifier means different things in
different contracts, and the contracts deliberately cannot confirm each
other's state. See
[ADR 0002](../../docs/adr/0002-scholarships-on-chain-boundaries.md) for why, and
for what that implies for anyone building an indexer or an operator dashboard.

**Consequence worth internalising:** a successful call in one contract is not
evidence that the corresponding state exists in another. If your tooling
asserts "award reserved, therefore budget decremented", that is a join you
have to perform yourself, off-chain, and it can be wrong.

## Roles and authorisation

Roles live in `scholarship-registry` and are granted by the registry admin:

| Role | Held by | Grants |
| --- | --- | --- |
| `Student` | Applicants | Nothing directly; the registry records the affiliation |
| `Sponsor` | Programme owners | Program administration in `scholarship-core` |
| `Reviewer` | Reviewers | Application assignment and review decisions |
| `Finance` | Finance staff | Disbursement creation and execution |
| `Administrator` | Contract admins | `initialize`, role and module wiring, budget configuration |

Two different things are both called "admin", and confusing them wastes time:

- The **contract admin** passed to `initialize` is a single address that can
  re-initialise, add modules, grant roles, and configure budgets.
- The **program owner** (`program.owner`, set by `create_program`) is the only
  address that may `transition_program`. Acting as the wrong one produces
  `NotProgramOwner`, not `NotAdmin`.

## Applicant guide

An applicant never calls `initialize` and never holds a contract role. The
whole journey is three steps.

### 1. Consent to the published terms

The sponsor publishes terms; the applicant accepts whatever is current:

```
publish_consent_terms(admin, program_id, terms_hash) -> u32   // sponsor
record_consent(applicant, program_id)                   -> u32   // applicant
revoke_consent(applicant, program_id)
get_latest_consent_version(program_id)          -> u32
has_valid_consent(applicant, program_id)        -> bool
```

`publish_consent_terms` **auto-increments** the version and returns it. You do
not choose a version, and there is no way to publish terms for a specific
number. `record_consent` likewise takes no version: it records consent to the
**current** version and returns it.

That design has one consequence worth being explicit about: you cannot consent
in advance to terms that are not yet published, and you cannot pre-consent to a
future version. If a sponsor publishes new terms while your application is in
flight, your recorded consent is behind the current version and the submission
is refused with `ConsentOutOfDate` until you call `record_consent` again. That
is the mechanism that makes new terms binding, and it is deliberate.

`revoke_consent` withdraws at any time. A revoked consent is not a deletion —
the record remains with a revoked flag, because an auditable "they withdrew" is
different from an absent row.

### 2. Submit the application

```
submit_application(applicant, program_id, data_hash, form_version, consent) -> ()
```

`data_hash` is a 32-byte **commitment you compute off-chain** over the real
answers and documents. The contract never sees them. See
[Privacy](#privacy) before deciding what to put in it.

`consent` is a boolean assertion that you have read and accept the terms. It is
not a substitute for `record_consent`: the contract separately loads your stored
`ConsentRecord` and requires it to be unrevoked **and** at the program's current
terms version. Passing `true` without a current record fails on the record, not
on the flag.

Before you submit, confirm:

- The program is `Published` and the window is open. `ProgramInactive` or
  `DeadlinePassed` otherwise.
- A form schema is published and you are submitting against a real
  `form_version`. `NoFormSchemaPublished` or `FormSchemaNotFound` otherwise.
- Your consent is current. `ConsentRequired` or `ConsentOutOfDate` otherwise.

One application per program. A second attempt is `DuplicateApplication`; there
is no edit or replace path.

### 3. Track status

```
get_application(applicant, program_id)  -> status, submitted_at, versions
has_applied(applicant, program_id)      -> bool
```

**`status` is always `Submitted`.** `ApplicationStatus` declares a single
variant and no function mutates it. There is no `UnderReview`, `Accepted` or
`Rejected` state, and no review-outcome function anywhere in the scholarship
contracts.

So an application cannot be marked accepted or rejected on-chain, and any
dashboard rendering a per-application decision is rendering something it
computed itself. See [Known gaps](#known-gaps) before building against that.

Application content is not retrievable from chain at any point; keep your own
copy of what you submitted, because the commitment is one-way.

## Sponsor guide

### Create the program and keep it legal

```
create_program(owner, program_id, sponsor_id, title, description, currency, funding_model)
transition_program(caller, program_id, to)                    -> ()
get_program_status(program_id)                               -> ProgramStatus
```

The creating address becomes `program.owner` and is the only address that may
`transition_program` — note the first parameter is `owner`, not `admin`.
`funding_model` is one of `FixedAward`, `MatchingFund` or `MilestoneBased`, and
`currency` is a short symbol such as `USDC`.

Lifecycle states are `Draft`, `Published`, `Paused`, `Closed`, `Archived`. The
only legal transitions are:

| From | To |
| --- | --- |
| `Draft` | `Published`, `Archived` |
| `Published` | `Paused`, `Closed` |
| `Paused` | `Published`, `Closed` |
| `Closed` | `Archived` |

Anything else is `InvalidTransition`. Note what is **not** in that table: there
is no path out of `Archived`, and no path from `Closed` back to `Published`. If
you close a program by mistake it is finished; open a new one.

Use `Paused` for a temporary stop. It preserves the record and can be resumed,
which `Closed` cannot.

`get_program_status` reports the current state, but the contract stores only
the latest transition. The full history is reconstructable from `PROGTRAN`
events, so **an indexer is required** if you need the audit trail.

### Open the application window

```
set_program_window(admin, program_id, opens_at, closes_at,
                   timezone_offset_minutes, grace_period_seconds)
is_open(program_id)             -> bool
is_within_grace(program_id)     -> bool
```

`opens_at` and `closes_at` are absolute ledger timestamps (`u64`); out-of-range
values give `WindowOverflow`. `InvalidWindow` means the boundaries are
inconsistent — typically `closes_at` before `opens_at`.

`grace_period_seconds` is a **duration, not an end timestamp**, and
`timezone_offset_minutes` records the program's intended offset so tooling can
present local times without guessing. Neither affects acceptance: `opens_at`
and `closes_at` are compared against ledger time.

The grace period is what lets a review that started before the deadline finish
after it. It does not extend submission: `submit_application` refuses with
`DeadlinePassed` once the window closes, regardless of grace.

### Configure the award budget

```
configure_award_budget(admin, program_id, max_recipients, per_award_amount, total_budget)
reserve_award(admin, program_id)                              -> u32
release_award(admin, program_id)
get_award_budget(program_id)
remaining_capacity(program_id) -> u32
```

All three values must be positive, and
`max_recipients * per_award_amount` must not exceed `total_budget`, or
`InvalidBudgetConfig`. That product is the ceiling: because `reserve_award`
refuses once `awarded_count` reaches `max_recipients`, committed funds can
never approach `total_budget` on their own. As a result the declared
`BudgetExceeded` error is effectively unreachable while the configure-time
invariant holds — it is defence in depth behind `InvalidBudgetConfig`.

`reserve_award` commits **one** award at the configured `per_award_amount` and
returns the new `awarded_count`. It takes no amount argument: the per-award
amount is whatever was configured. `release_award` gives one back and returns
nothing; `NoAwardsReserved` means `awarded_count` was already zero, which is
usually a double release rather than an accounting problem.

> **Do not re-run `configure_award_budget` once awards exist.** There is no
> guard against it. The call overwrites the stored budget wholesale and resets
> `awarded_count` and `committed_amount` to `0`, so the reservation history is
> discarded and the `total_budget` ceiling can be circumvented for the life of
> the program: reserve against one configuration, re-configure to zero the
> counters, then reserve again. The `AWDRSV` event stream is the only durable
> record of what was reserved. Treat configuration as a one-time setup step and
> gate it operationally, because the contract will not.

## Reviewer guide

### Application review

What exists in `scholarship-applications` is reviewer *administration and
assignment*:

```
register_reviewer(admin, reviewer, max_assignments)
set_reviewer_active(admin, reviewer, active)
set_reviewer_pool(admin, program_id, reviewers)
set_review_mode(admin, program_id, mode)   -> Open | Blind | DoubleBlind
declare_conflict(reviewer, program_id, applicant_commitment)
has_conflict(reviewer, program_id, applicant_commitment)  -> bool
assign_reviewer(admin, program_id, application_commitment, mode)  -> AssignmentMode
get_assignment(program_id, application_commitment)
```

`max_assignments` caps how many applications one reviewer may hold, and
`set_reviewer_active` takes a reviewer out of the pool without revoking their
registration.

Applications are identified by `application_commitment: BytesN<32>` rather than
by applicant address. That is what makes blind review possible: the commitment
can be assigned and conflicted against without naming the applicant.
`assign_reviewer` takes an `AssignmentMode` — `RoundRobin`, `LoadBalanced` or
`SeededRandom(u64)` — and the seeded variant takes a seed, so a disputed
allocation can be reproduced exactly.

**There is no function that records a review decision.** No
`set_decision`, `record_review` or equivalent exists, and `ApplicationStatus`
never leaves `Submitted`. The only place a decision is recorded anywhere in the
scholarship contracts is `scholarship-milestones.verify_evidence`.

So application review is assign-and-declare-only on-chain today: you can staff
and allocate a panel, but the outcome is produced off-chain.

`set_review_mode` currently records the intended `Open` / `Blind` /
`DoubleBlind` disclosure rule and **has no on-chain consumer**. There is no
review event to disclose — `scholarship-applications` publishes only `CONSENT`,
`CONSPUB`, `FORMPUB` and `SUBMIT` — and no decision point at which to unseal.
`get_review_mode` will faithfully return what you set; nothing else reads it
today, so the blinding rule must be enforced by whatever off-chain review flow
actually shows reviewers.

Declare conflicts **before** accepting an assignment. `has_conflict` is a
queryable fact, and a conflict discovered after a decision is a governance
incident, not a bug.

### Milestone evidence

```
create_milestone(admin, program_id, milestone_id)
set_milestone_status(admin, program_id, milestone_id, status)
submit_evidence(submitter, recipient, program_id, milestone_id, data_hash) -> u32
verify_evidence(verifier, program_id, milestone_id, recipient, version, decision, reason)
get_evidence(program_id, milestone_id, recipient, version)
```

Evidence is versioned: `submit_evidence` returns the new version, and
`verify_evidence` names the exact version being judged. Verifying a version
that is not the latest is not blocked, so the caller must check
`latest_evidence_version` first if it means to judge the current submission.

`reason_code` is mandatory and enumerated. A free-text reason is rejected
(`InvalidReasonCode`), which keeps refusals machine-readable for appeals.

## Finance guide

### Wallet verification before any payout

Payouts are not sent to an address you were handed. Each recipient proves
control of a payout wallet:

```
set_payout_wallet(recipient, wallet)
register_wallet_key(wallet, pubkey)            -> BytesN<32>
open_wallet_challenge(recipient, wallet, ttl_seconds) -> u64
wallet_challenge_payload(challenge_id)         -> BytesN<32>
confirm_wallet_challenge(challenge_id, pubkey, signature)
is_wallet_verified(recipient)                  -> bool
```

The challenge payload is signed off-chain and verified on-chain. A wallet is
only usable once `is_wallet_verified` is true; a challenge that is expired
(`ChallengeExpired`), already spent (`ChallengeAlreadyUsed`), or presented for
a different wallet (`ChallengeWalletMismatch`) is refused.

Choose `ttl_seconds` short. The challenge exists to be answered promptly, and a
long-lived challenge is a replay target. `InvalidChallengeTtl` is the contract
refusing a window outside its accepted range.

### Create and execute an intent

```
add_creator(admin, creator)                     -> authorise a finance address
create_intent(creator, program_id, recipient, amount, installment) -> BytesN<32>
is_intent_executable(intent_id)                 -> bool
record_execution(executor, intent_id)
cancel_intent(caller, intent_id)```

`create_intent` is deliberately not a transfer. It records an obligation that
`record_execution` settles later, which gives finance a window to satisfy
preconditions and a place to record cancellation.

Check `is_intent_executable` before `record_execution`. Recording twice is
`AlreadyExecuted`; an intent whose state has moved on is `IntentConflict`. A
cancelled intent is `AlreadyCancelled` and is terminal.

`InvalidAmount` and `Overflow` are the arithmetic guards. An `Overflow` here
means the running total would exceed what the amount type can hold — treat it
as a stop-and-escalate, not a retry.

## Privacy

### What is on-chain

Applications store a commitment, not content. The `Application` record is:

| Field | Value |
| --- | --- |
| `applicant` | The applicant's address |
| `program_id` | The program |
| `status` | Current status |
| `submitted_at` | Ledger timestamp |
| `data_hash` | Your 32-byte off-chain commitment |
| `form_version` | Version consented to |
| `consent_version` | Version of terms consented to |

No answer, document, name, or amount of personal data is stored. Consent
records likewise hold only a version number, timestamps, and a revoked flag.

### What the event stream reveals anyway

Storage privacy and event privacy are different things, and the ledger is
public. Verified against the current sources:

- `SUBMIT` publishes `(applicant, program_id)`
- `CONSENT` publishes `(applicant, program_id, version)`
- `ATTEST` publishes `(issuer, subject, attestation_type)`

So while the *content* of an application stays private, **participation does
not**. Anyone reading the ledger can learn who applied to which program, and
who holds which eligibility attestation. This is a deliberate trade: an indexer
that cannot associate a submission with a program cannot build a candidate's
award status, which is the point of the platform.

If your threat model does not tolerate that disclosure, it is a product
decision rather than a bug, and it starts at those three publish sites.

### Choosing `data_hash`

`data_hash` is caller-supplied. Two rules:

1. It must be a real commitment over content you actually want to prove later.
   A constant is not a commitment.
2. It must not itself leak. A bare hash of a small, guessable answer space is
   reversible by enumeration. Salt it, or commit to a document bundle rather
   than a field.

## Known gaps

Verified against the current sources. These are absent features, not bugs in
working features, and each one is a place where an off-chain flow is currently
carrying weight the chain does not.

| Gap | Evidence | Consequence |
| --- | --- | --- |
| No application review outcome | `ApplicationStatus` has one variant; nothing mutates it; no decision function in any contract | Application accept/reject is off-chain; on-chain status is always `Submitted` |
| `set_review_mode` has no consumer | Only `CONSENT`, `CONSPUB`, `FORMPUB`, `SUBMIT` are published | `Blind`/`DoubleBlind` blinding is not enforced anywhere on-chain |
| Award→disbursement is not linked | `reserve_award` is in `scholarship-programs`, `create_intent` is in `scholarship-disbursements` | Funds can be reserved with no intent, and intents created against no reservation |
| Milestones are not linked to programs | `create_milestone` performs no program existence check | Milestones can be created against programs that never existed |
| Budget can be silently reconfigured | `configure_award_budget` has no state guard and resets `awarded_count`/`committed_amount` to `0` | Re-running it mid-program discards reservation history and can circumvent `total_budget` |
| No on-chain pause or stop | Only the `Published` → `Paused` program state exists, and it is per-program | No global kill switch; pausing is per-program and reversible |
| Declared `BudgetExceeded` is unreachable | `InvalidBudgetConfig` enforces `max_recipients * per_award <= total_budget` at configure time, and `reserve_award` refuses at `max_recipients` | Dead error, and defence in depth that only holds while configuration is not re-run |

The award→disbursement gap is the one most likely to cause money to be missed
in either direction. Reconciling reservations against executed intents has to
be an explicit off-chain process, because the chain cannot tell you which
intents correspond to which reserved award.

The budget reconfiguration gap is the one most likely to cause money to be
over-committed, because it removes the only ceiling on total committed funds.
Treat `configure_award_budget` as privileged and gate it out of any routine
operations tooling.

## Troubleshooting

| Error | Meaning | Do this |
| --- | --- | --- |
| `NotInitialized` | Admin-only call before `initialize` | Check deploy order; `initialize` is once-only |
| `AlreadyInitialized` | `initialize` called twice | Expected; ignore. Init is not idempotent |
| `NotAdmin` | Caller is not the contract admin | You are probably the *program owner* instead — see below |
| `NotProgramOwner` | Caller does not own the program | Check `program.owner`; ownership is not transferable in-contract |
| `ProgramNotFound` | No such program in *this* contract | Cross-contract joins do not hold; verify the ID in the contract you called |
| `ProgramInactive` | Program not `Published` | Owner must `transition_program` to `Published` |
| `DeadlinePassed` | Window closed | Grace does not extend submission |
| `WindowNotFound` | No window configured | `set_program_window` was never called |
| `InvalidWindow` | Inconsistent window bounds | Check ordering: open, close, grace |
| `InvalidBudgetConfig` | Zero/negative value, or `max_recipients * per_award > total_budget` | Lower the per-award amount or raise the total |
| `CapacityExceeded` | `awarded_count` has reached `max_recipients` | Check `remaining_capacity`; the program is fully awarded |
| `BudgetExceeded` | Committed funds would pass `total_budget` | **Unreachable via normal use.** If you see it, `configure_award_budget` was re-run after reservations — see [Known gaps](#known-gaps) |
| `NoAwardsReserved` | `awarded_count` was already zero | Likely a double release; reconcile against `AWDRSV` events first |
| `DuplicateApplication` | Already applied to this program | One application per program; there is no edit path |
| `ConsentRequired` | No consent recorded | `record_consent` for the current version |
| `ConsentOutOfDate` | New terms published since your consent | Re-consent to the new version |
| `FormSchemaNotFound` | No such form version | Check `get_latest_form_version` |
| `RuleNotFound` / `NoRulePublished` | Eligibility rule missing | Distinguish "never published" from "no such version" |
| `VersionOverflow` | Counter at `u32::MAX` | Not reachable in practice; report if seen |
| `ChallengeExpired` | Wallet challenge timed out | Re-open a challenge with a short TTL |
| `AlreadyExecuted` | Intent already settled | Terminal; check `get_intent` before retrying |
| `IntentConflict` | Intent state moved on | Re-read the intent before acting |

### Reads that look like bugs but are not

Before initialising a contract, several read paths report a *not found* error
rather than `NotInitialized`. In `scholarship-programs`, `get_program_window`
returns `WindowNotFound`; in `scholarship-registry`, `get_module` returns
`ModuleNotFound`. Both readings are true, and the second is more actionable
when you are debugging a fresh deployment. This is documented rather than
changed, because adding a read guard would alter the ABI.

## Incident actions

### A program is open when it should not be

The fastest safe move is `transition_program` to `Paused`. It preserves the
record, stops new submissions, and is reversible — unlike `Closed`.

```
transition_program(<owner address>, program_id, Paused)
```

### A bad form version or consent version was published

Versions are immutable. Publishing a corrected version is additive; existing
applications stay bound to the version they were submitted against. Expect
`ConsentOutOfDate` on the next submission until applicants re-consent, which
is the intended behaviour rather than a regression.

### A payout wallet needs to change

`set_payout_wallet` replaces it, which invalidates the previous verification.
Expect `is_wallet_verified` to go false and require a fresh challenge before
the next intent executes. Do not skip the challenge to save time.

### An interface change needs to ship

The public interface is gated. See
[`.github/workflows/scholarship-compat.yml`](../../.github/workflows/scholarship-compat.yml)
and `scripts/scholarship_surface.py`. A breaking change — a removed function, a
reordered parameter, a renamed event topic, a changed payload arity — fails CI
unless it is listed in
[`.github/scholarship-compat-override.json`](../../.github/scholarship-compat-override.json)
with a `migration` note, in the same PR as the change.

Deployed contracts cannot be un-deployed. There is no pause switch and no
rollback in-contract; recovery is a migration, which is why the gate exists
before deployment rather than after.

## Related documents

| Document | Covers |
| --- | --- |
| [scholarship-core.md](scholarship-core.md) | Program records, lifecycle, storage |
| [scholarship-registry.md](scholarship-registry.md) | Module wiring and roles |
| [scholarship-programs.md](scholarship-programs.md) | Windows and budgets |
| [scholarship-eligibility.md](scholarship-eligibility.md) | Rules and attestations |
| [scholarship-applications.md](scholarship-applications.md) | Forms, consent, submission |
| [events.md](events.md) | Event catalogue across all contracts |
| [indexer-projection-schema.md](indexer-projection-schema.md) | How to build a consistent projection |
| [ADR 0002](../../docs/adr/0002-scholarships-on-chain-boundaries.md) | Why the contracts do not call each other |
| [scholarship-staging.md](scholarship-staging.md) | Provisioning, verifying, and reversing a staging scenario |
| [scholarship-launch-readiness.md](scholarship-launch-readiness.md) | Go-live criteria, hard blockers, and rollback |
