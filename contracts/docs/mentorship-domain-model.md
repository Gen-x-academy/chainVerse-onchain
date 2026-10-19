# Mentorship Bounded Contexts and Vocabulary

Shared Soroban types: `contracts/mentorship-types`  
Version: 1

## Canonical terms

| Term | Meaning | Canonical identifier |
|---|---|---|
| Mentor | Service provider responsible for delivering the engagement | `MentorId(Address)` |
| Mentee | Participant receiving mentorship; `Student` in the existing access-control ABI | `MenteeId(Address)` |
| Program | Published offering, eligibility and participation rules | `MentorshipProgramId(BytesN<32>)` |
| Engagement | One accepted relationship between one mentee and one mentor under a program | `EngagementId(BytesN<32>)` |
| Session | A scheduled occurrence belonging to one engagement | `SessionId(BytesN<32>)` |
| Deliverable | A committed artifact or outcome attached to an engagement/session | `DeliverableId(BytesN<32>)` |
| Payment | Escrowed token obligation associated with an engagement | `PaymentId(BytesN<32>)` |
| Dispute | A formal challenge to an engagement, session, deliverable, or payment outcome | `DisputeId(BytesN<32>)` |
| Currency | A deployed Soroban token contract, never a display ticker | `CurrencyId(Address)` |
| Content commitment | Hash of private off-chain content | `ContentCommitment(BytesN<32>)` |
| Token amount | Integer in the token's smallest denomination | `TokenAmount(i128)` |

The existing access-control roles remain stable: `Student` is the Mentee
authorization role; `Tutor` is a provider permission profile; `Mentor` is the
broader provider role. A person may hold both `Tutor` and `Mentor` grants.
Identity verification, credentials, names, and profile content are not encoded
in these identifiers.

## Bounded contexts and ownership

| Context | Owns | Does not own |
|---|---|---|
| Program & configuration | Program publication/pause/closure; environment capability limits in `mentorship-config` | Participant authorization, session evidence, or token movement |
| Identity & authorization | Address-bound roles and permission checks in `mentorship-access-control` | Mentorship profile contents, credentials, or program lifecycle |
| Engagement | Accepted mentor/mentee relationship, agreed terms commitment, and engagement lifecycle | Private agreement text, session content, or escrow balances |
| Session | Schedule, attendance/completion status, and public attestations | Private notes, call recordings, or session transcripts |
| Deliverable | Submission/revision/acceptance status and content commitment | Artifact bytes or private feedback |
| Payment | Token, amount, funding, release/refund, and escrow linkage | Session quality judgments or private evidence |
| Dispute | Case status, resolution outcome, and settlement instruction | Safeguarding reports or private case narrative |

An engagement references its program, mentor, and mentee; sessions and
deliverables reference their parent engagement. Payment owns the authoritative
token state. A dispute may block or redirect payment only through an explicit
payment/escrow transition. Safeguarding and moderation remain separate
processes and do not automatically change escrow state.

## Lifecycle states

The shared type crate provides the following state machines. Transitions not
listed by `can_transition_to` are invalid.

| Aggregate | States |
|---|---|
| Program | Draft -> Published -> Paused -> Published or Closed; Published may also close |
| Engagement | Requested -> Accepted -> Active -> Completed; cancellation is allowed before completion; Active may enter Disputed, which resolves to Completed or Cancelled |
| Session | Scheduled -> InProgress -> EvidenceSubmitted -> Confirmed; cancellation is allowed before evidence; InProgress/EvidenceSubmitted may enter Disputed |
| Deliverable | Draft -> Submitted -> Accepted, Rejected, RevisionRequested, or Disputed; RevisionRequested may be resubmitted, rejected, or cancelled |
| Payment | PendingFunding -> Funded or Cancelled; Funded -> PartiallyReleased/Released/Refunded/Disputed; dispute resolution may release, refund, or partially release |
| Dispute | Open -> UnderReview -> Resolved; Open or UnderReview may be Withdrawn |

Payment transitions must additionally preserve the escrow invariant that total
released plus refunded amounts never exceed funded amount. `DisputeOutcome`
records the adjudication; the payment contract separately performs the
corresponding settlement transition.

## Cross-context invariants

- Identifiers are opaque and immutable after creation; no PII or human-readable
  names are embedded in IDs.
- An engagement binds exactly one mentee, one mentor, and one program. A
  participant's role grant does not itself create an engagement.
- Sessions, deliverables, payments, and disputes must reference an existing
  parent engagement; child state changes cannot independently reopen a
  terminal engagement.
- Agreement terms and submitted deliverable/evidence content remain off-chain.
  On-chain values are hashes/references, state, amounts, timestamps, and public
  attestations only.
- A feature flag enables a capability but never grants authorization. Each
  state-changing call authenticates the actor and checks the owning context's
  policy.
- Amounts are non-negative token base units paired with a token contract
  address; floating-point amounts and ticker-only currency identifiers are
  forbidden.
- An accepted engagement freezes its agreed terms commitment. Changes require
  an explicit amendment/version rather than overwriting prior terms.
- Program configuration is prospective. Existing engagement terms and
  settlement obligations are not silently changed by a configuration update.

## ABI and privacy

The `mentorship-types` crate exports Soroban `contracttype` newtypes, roles,
outcomes, and lifecycle enums for use by mentorship contracts. These are
vocabulary and validation primitives; they do not create or store engagements.
Session evidence, deliverable bytes, profiles, and dispute narratives remain
off-chain. Contracts should store only the commitment/reference and public
status needed for integrity, authorization, or settlement.