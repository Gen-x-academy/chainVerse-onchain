# Scholarship Awards Contract

- Status: Implemented
- Owner: Scholarships On-chain working group
- Related issues: #1090 (award records and acceptance deadlines), #1091
  (signed agreement acceptance), #1092 (cancellation and termination)
- Depends on / relates to: ADR 0002 (`docs/adr/0002-scholarships-on-chain-boundaries.md`),
  `contracts/scholarship-milestones` (#1093)

## Scope

This contract (`contracts/scholarship-awards`) implements the
"Scholarships On-chain/Awards" epic's three behaviors:

- **#1090 — Create award records and acceptance deadlines.** `create_award`
  materializes an approved award with its `amount`, `currency`, immutable
  `terms_version`/`terms_hash`, optional `milestone_schedule_id`, and an
  expiring `acceptance_deadline`. A `(recipient, program_id)` uniqueness
  key (`has_active_award`) is the "conflicting awards" guard: an applicant
  can never hold two active awards for the same program. Each active award
  holds one budget reservation per program (`get_reservation`), and every
  decline/expiry/cancellation/termination releases it.
- **#1091 — Capture signed award agreement acceptance.** `accept_award`
  requires the recipient's own signature, refuses a closed window, and
  stores an `AgreementAcceptance` that copies the offer's terms version and
  hash, the signer address, the ledger timestamp, and an integrity
  commitment to the required declarations. Terms are immutable, so a later
  terms change can never retroactively alter what was agreed to.
  `can_disburse` is `false` for any declined or expired offer.
- **#1092 — Handle cancellation and termination.** A single
  `is_legal_transition` table gates every status change. `cancel_award` is
  pre-payment only (`PaymentAlreadyMade` once a disbursement exists);
  `terminate_award` is post-payment only (`NoPaymentMade` otherwise). Both
  stop all future payments (further `record_disbursement` calls fail with
  `AwardNotDisburseable`), record the reason code and deciding authority,
  and — for termination — store any recovery in a dedicated `Recovery`
  record kept separate from `paid_amount`, so the two are always
  independently auditable. Every terminal transition emits an event
  carrying the recipient address: the on-chain half of the "safe notice"
  requirement, with the actual notification left to off-chain services
  (see Operational impact).

**Not implemented here:** actually moving funds. `record_disbursement`
records that a payment happened elsewhere — it is accounting only, so that
#1092 can distinguish pre-payment cancellation from post-payment
termination. Wiring real token transfers in is follow-up work and, per ADR
0002's disbursement non-goal, should be reviewed against the existing
`payment`/`payout-automation`/`escrow`/`escrow-vault` contracts first.

## State machine

```
Offered ──accept──▶ Accepted ──record_disbursement──▶ Accepted (paid_amount > 0)
   │                    │                                    │
   ├─decline──▶ Declined (terminal)                          ├─cancel (pre-payment)──▶ Cancelled (terminal)
   ├─expire───▶ Expired  (terminal)                          └─terminate (post-payment)▶ Terminated (terminal)
   └─cancel───▶ Cancelled (terminal)
```

`Declined`, `Expired`, `Cancelled`, and `Terminated` are terminal — there
is no transition out of, or between, them.

## Privacy

On-chain storage never holds award terms or declaration documents. Each is
represented only by a caller-supplied `BytesN<32>` integrity commitment
(`terms_hash`, `declarations_hash`) over off-chain content. The only
addresses stored are the award recipient and the stored admin — never an
applicant's personal data.

## Ownership

The admin (`initialize`) exclusively creates awards, records
disbursements, cancels, and terminates. The recipient alone can accept or
decline an offer — `require_auth` is enforced on their own address, so no
admin or relayer can sign an agreement on their behalf. `expire_award` is
permissionless (anyone may finalize an unanswered offer once the deadline
has passed), but it can only move an `Offered` award to `Expired`, so it
grants no authority over the recipient.

## Migration

New contract, no prior on-chain state. After deployment, the admin must
`initialize` before any award can be created. Award IDs are caller-supplied
and permanent — there is no rename or re-key operation.

## Operational impact

- **Storage/TTL.** Awards, acceptances, active-award pointers, reservation
  counters, and recovery records use `persistent` storage with a ~1-year
  TTL bump on write/read (matching the sibling scholarship contracts).
- **Events.** `AWCRT`, `AWACC`, `AWDEC`, `AWEXP`, `AWCAN`, `AWTRM`, and
  `AWDIS` carry the award ID plus the actor/recipient, so an off-chain
  indexer can reconstruct the full lifecycle — including terminal
  transitions the record no longer distinguishes — uniformly with the rest
  of the epic.
- **Notice.** "Recipients receive safe notice" is satisfied on-chain by
  emitting the terminal event with the recipient address; actually
  delivering a notification (email/in-app) is an off-chain responsibility
  that must subscribe to these events.
- **Reservation semantics.** A program's reservation is released on every
  terminal transition, including post-payment termination. Awards that run
  to completion stay `Accepted` and keep their reservation; if that
  matters for long-lived programs, an explicit "complete" action (releasing
  the reservation while preserving the record) is a natural follow-up.
- **Cross-contract wiring.** This contract does not call
  `scholarship-programs`' `reserve_award`/`release_award`; its reservation
  counter is local. Wiring the two together so a program's budget ceiling
  is authoritative across both contracts is follow-up work, in line with
  ADR 0002's "cross-contract enforcement" non-goal.
