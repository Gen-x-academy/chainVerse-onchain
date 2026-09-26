# Scholarship launch readiness and rollback

Go/no-go criteria for putting the scholarship contracts in front of real
applicants and real money, and what to do when something goes wrong.

This document covers **scholarship-specific** readiness. The mechanics of
upgrading and rolling back a contract — canary phase, health gates, rollback
references, evidence capture — are already specified in
[runbook-canary-upgrade.md](../../docs/runbook-canary-upgrade.md). Read that
first; this document tells you whether to start, and what rollback means for
applications and funds specifically.

For what the contracts do, see [scholarship-guide.md](scholarship-guide.md).
For provisioning a test scenario, see
[scholarship-staging.md](scholarship-staging.md).

## The short version

The contracts are not ready to launch, and the reasons are specific and
verifiable rather than a general sense of caution. Five of them are
[hard blockers](#hard-blockers) — gaps where the chain does not do something
the product needs. Each was confirmed against the current sources rather than
inferred from the docs.

## Hard blockers

Do not launch until each of these has an explicit, written resolution. Most are
resolved by deciding that something happens off-chain and documenting it, but
that decision has to be made consciously by someone who owns it.

### 1. There is no on-chain application review outcome

`ApplicationStatus` declares a single variant, nothing mutates it, and no
contract records a review decision. Only milestone evidence has a recorded
decision (`verify_evidence`).

**Consequence:** accept/reject is off-chain and unreproducible from the ledger.
An applicant who is rejected has no on-chain record of why, and a dispute cannot
be settled from chain state alone.

**Resolution needed:** name the system that holds the decision, define its
retention period, and state that the ledger is not the system of record. If that
is unacceptable, the missing function is a code change, not a process.

### 2. Blind review is not enforced anywhere

`set_review_mode` records the intended rule and has **no on-chain consumer**.
`scholarship-applications` publishes only `CONSENT`, `CONSPUB`, `FORMPUB` and
`SUBMIT`. There is no review event to disclose and no decision point at which to
unseal.

**Consequence:** `Blind` and `DoubleBlind` are a promise made by a field nobody
reads. If it is not enforced by the review tool, an operator running a
double-blind panel will leak identities by accident.

**Resolution needed:** either enforce blinding in the review tooling and
document it as a security control, or stop offering the modes. Shipping
`DoubleBlind` with no enforcement is worse than not offering it, because it
creates documented expectation without a mechanism.

### 3. The award budget can be silently reset

`configure_award_budget` has no state guard. It overwrites the stored budget and
resets `awarded_count` and `committed_amount` to `0`.

**Consequence:** any account with admin rights can re-run it mid-program,
discarding reservation history and circumventing the `total_budget` ceiling for
the life of the program. The `AWDRSV` event stream is the only durable record of
what was reserved.

**Resolution needed:** treat the function as privileged. Restrict the admin key,
gate the function out of routine operations tooling, and alert on any
`configure_award_budget` event after the first. A code guard would be better.

### 4. Award reservations and disbursements are not linked

`reserve_award` lives in `scholarship-programs`; `create_intent` lives in
`scholarship-disbursements`. Neither checks the other. This is a deliberate
consequence of
[ADR 0002](../../docs/adr/0002-scholarships-on-chain-boundaries.md).

**Consequence:** funds can be reserved with no intent, and intents can be
created against no reservation. The chain cannot tell you which intents
correspond to which award, so over-commissioning and under-payment are both
possible and neither is visible on-chain.

**Resolution needed:** a reconciliation process that joins reservations to
intents off-chain, run on a schedule, with a named owner. Define the expected
cadence and the alert threshold for a mismatch before launch, not after.

### 5. Records expire in 36 days

Every contract sets `RECORD_MIN_TTL = 3_110_400` (36 days) and
`RECORD_MAX_TTL = 6_220_800` (72 days) on its persistent entries.

**Consequence:** an application written on day 1 is guaranteed readable for 36
days. A scholarship program with a long application window, or a review backlog,
can outlive its own records. Expired entries need an explicit restore, which
costs a transaction and can fail.

**Resolution needed:** measure the longest realistic gap between submission and
resolution. If it approaches 36 days, a TTL bump job is mandatory before launch,
and it needs an owner and a funding source.

## Measurable go/no-go criteria

Every row is a threshold, not a sentiment. "Owner" is a role; the launch is
blocked until a named person holds each one.

### Security

| # | Criterion | Measure | Pass condition | Owner |
| --- | --- | --- | --- | --- |
| S1 | Contract audit | External audit report | Report exists; no unresolved high or critical findings | Platform lead |
| S2 | CI green | `contracts.yml` on the release commit | All jobs pass, including the scholarship compatibility gate | Platform lead |
| S3 | Mainnet reproducible | Clean-room build of the release commit | WASM hash matches the audited artefact byte for byte | Release manager |
| S4 | Admin key custody | Key policy | Threshold multisig, documented holders, tested recovery | Security owner |
| S5 | Budget function gated | Access review | `configure_award_budget` unreachable from routine tooling | Security owner |
| S6 | Blinding enforced | Review-tool audit | Tool demonstrably withholds identity in `Blind`/`DoubleBlind` | Product owner |

### Privacy

| # | Criterion | Measure | Pass condition | Owner |
| --- | --- | --- | --- | --- |
| P1 | No PII on-chain | Storage review of all seven contracts | Only addresses, IDs, versions, timestamps, hashes, and amounts | Privacy reviewer |
| P2 | Data hash salted | Review of the application form | Commitment is salted or over a document bundle, not a bare low-entropy field | Product owner |
| P3 | Retention stated | Published privacy notice | States what is kept off-chain, for how long, and who can read it | Privacy reviewer |
| P4 | Participation disclosure | Notice and UI copy | States plainly that `SUBMIT`/`CONSENT`/`ATTEST` publish addresses | Privacy reviewer |

### Accessibility

| # | Criterion | Measure | Pass condition | Owner |
| --- | --- | --- | --- | --- |
| A1 | Application path | Keyboard-only walkthrough of the full journey | Completes with no pointer-only step | Design owner |
| A2 | Screen reader | VoiceOver/NVDA pass on application and status views | No blocker; errors announced | Design owner |
| A3 | Status legibility | Status is not colour-only | Every state distinguishable without colour | Design owner |
| A4 | Documented alternative | Support path | A non-wallet route to apply exists, or its absence is an accepted, documented gap | Product owner |

### Solvency

| # | Criterion | Measure | Pass condition | Owner |
| --- | --- | --- | --- | --- |
| F1 | Budget invariant | Audit `configure_award_budget` calls | `max_recipients × per_award ≤ total_budget` holds for every program | Finance owner |
| F2 | Funding confirmed | Treasury sign-off | Funds committed and the disbursement token address recorded | Finance owner |
| F3 | Reconciliation live | Dry run of the reservation↔intent join | Runs clean on staging; cadence and alert threshold documented | Finance owner |
| F4 | Wallet verification | Sample of recipients | Every payout wallet has a completed challenge | Finance owner |
| F5 | Rounding | Test of the smallest real award | No rounding loss or dust accumulation across a full cycle | Finance owner |

### Support

| # | Criterion | Measure | Pass condition | Owner |
| --- | --- | --- | --- | --- |
| U1 | Error catalogue | Map every `ContractError` to a user-facing message | No raw error reaches an applicant | Support lead |
| U2 | Runbook rehearsed | Tabletop exercise of one incident | Team completes it without improvising | Support lead |
| U3 | Escalation path | Published contacts | Named rota with response times | Support lead |
| U4 | Applicant recourse | Published process | A rejected applicant can learn why and how to appeal | Product owner |

`U1` is the one most likely to be skipped and most visible to users. The
troubleshooting table in [scholarship-guide.md](scholarship-guide.md#troubleshooting)
is the starting point.

### Monitoring

| # | Criterion | Measure | Pass condition | Owner |
| --- | --- | --- | --- | --- |
| M1 | Event indexing | Indexer consumes all 26 events | Every topic indexed with correct arity | SRE |
| M2 | Failed transaction alerting | Alert on error returns | Alert fires within one polling interval | SRE |
| M3 | TTL monitoring | Dashboard of oldest live entry | No application record below the 36-day floor un-actioned | SRE |
| M4 | Budget drift | Daily reserved vs disbursed | Difference inside the agreed tolerance | Finance owner |
| M5 | Wallet verification rate | Verified ÷ registered | Below threshold pages the finance owner | SRE |
| M6 | Upgrade canary | `runbook-canary-upgrade.md` rehearsed | Canary and rollback completed on testnet | SRE |

### Data migration

| # | Criterion | Measure | Pass condition | Owner |
| --- | --- | --- | --- | --- |
| D1 | No migration needed | Confirm first launch has no prior state | No production scholarship data exists to migrate | Platform lead |
| D2 | Interface frozen | Compatibility gate | Baseline updated deliberately, with a `migration` note per override | Platform lead |
| D3 | Re-consent path | Terms change rehearsal | `ConsentOutOfDate` → re-consent → resubmit works on staging | Product owner |
| D4 | Idempotent migration | Migration rehearsal | Re-running a completed migration is a no-op | Platform lead |

If any scholarship state already exists at launch, D1 becomes a real migration
project and this document is not sufficient.

## Rollback

### The good news: applications and funds survive a code rollback

Soroban rollback here means re-uploading the previous WASM to the **same
contract ID**. Ledger storage is untouched. So:

- Active applications survive. The `Application` records are still there.
- Reserved awards and their counters survive.
- Disbursement intents, wallet bindings, and attestations survive.
- The contract ID, and therefore every address and link already handed to an
  applicant, stays valid.

This is the single most important property for the "rollback preserves active
applications and funds" requirement, and it holds as long as the release did not
run a storage migration.

### The fast stop is per-program, not global

There is no global pause. The only emergency stop in the contracts is
`transition_program(owner, program_id, Paused)`, which:

- stops new submissions immediately,
- is reversible (`Paused → Published`),
- preserves the record, and
- applies to **one program**.

```bash
stellar contract invoke --id <core-id> --source <owner> --network mainnet -- \
  transition_program <owner-address> <program-id> Paused
```

For an incident affecting one misconfigured program, this is faster and safer
than a code rollback, and it is the correct first move. Roll back code only when
the *code* is the problem.

### When rollback is the wrong answer

Do not roll back if it would leave the contract unreadable. Per the existing
runbook, rollback cannot fix:

1. A storage migration that already ran — the old WASM cannot read the new keys.
2. Irreversible token transfers or burns already executed.
3. Events already consumed by an indexer or acted on by a user.
4. TTL changes already applied.

In those cases, do not roll back. Write a forward fix, and record the incident.

### Decision authority

| Situation | Action | Who decides |
| --- | --- | --- |
| One program misconfigured | `transition_program(..., Paused)` | Programme owner, immediately |
| Suspected contract defect, no data loss | Canary rollback | Release manager + one other signer |
| Suspected defect with funds at risk | Halt disbursements, then decide | Finance owner + Platform lead |
| Storage migration already applied | No rollback; forward fix | Platform lead, escalated |
| Key compromise | Halt, rotate, assess | Security owner, unilaterally |

The `Published → Paused` path should be rehearsed in the U2 tabletop. It is the
one action likely to be needed first, and the one most likely to be fumbled if
nobody has done it.

## Post-launch review

Scheduled, with a named owner and a date. A review that is not scheduled does
not happen.

| When | What | Owner |
| --- | --- | --- |
| **T+24h** | Error rate, failed transactions, first support contacts, any `Paused` program | SRE |
| **T+7d** | First full application cycle: submission → review → decision → disbursement, end to end | Product owner |
| **T+30d** | Solvency reconciliation, TTL monitoring, indexer completeness against all 26 events | Finance owner |
| **T+90d** | Full post-launch review: criteria above re-scored, hard blockers re-checked, gaps closed or re-accepted with a written rationale | Platform lead |
| **T+180d** | Follow-up: re-audit scope, retention review, decide whether the off-chain review system needs to move on-chain | Privacy reviewer |

Escalate immediately, without waiting for T+24h, if: a program is `Paused`, a
wallet verification is bypassed, a budget total is exceeded, or a rollback
occurs.

The T+90d review must explicitly re-score **hard blocker 1**. If the off-chain
review decision has become a real dependency — disputes, appeals, audit
requests — that is the signal to build the missing on-chain function rather
than continue to carry it.

## Ownership

| Area | Owner |
| --- | --- |
| The seven contract sources and the interface gate | Platform team |
| Audit and its findings | Security owner |
| Treasury, budget configuration, reconciliation | Finance owner |
| Review tooling and blinding enforcement | Product owner |
| Indexing, alerting, TTL monitoring, canary | SRE |
| Error messages, escalation, applicant recourse | Support lead |
| Retention and disclosure notices | Privacy reviewer |
| This document and the sign-off | Platform lead |

The sign-off is not complete until every role above has a named person and every
criterion has a recorded pass or an accepted, written exception.

## Privacy

This document contains no applicant data. Two launch-relevant points carry over
from [scholarship-guide.md](scholarship-guide.md#privacy):

- Application **content** stays off-chain by design; only commitments are stored.
- Application **participation** is public, because `SUBMIT`, `CONSENT` and
  `ATTEST` publish addresses.

Criterion P4 exists because the second point is easy to miss and hard to walk
back once applicants have been told otherwise.

## Migration

Launch is assumed to be a first launch: no production scholarship data, so no
migration. D1 verifies that assumption rather than trusting it.

If it turns out false, this document is not sufficient. Migrating live
applications and reserved funds needs its own design, rehearsal, and rollback
plan, and the compatibility gate
([`.github/scholarship-compat-override.json`](../../.github/scholarship-compat-override.json))
should record the `migration` note for the change that carries it out.

## Operational impact

- **Launch gate:** five hard blockers, 33 measurable criteria across seven
  areas, each with a named owner.
- **Ongoing:** the monitoring criteria are standing obligations, not a one-time
  check. M3 in particular is a recurring job with a real cost.
- **Incident cost:** a per-program `Paused` is minutes. A code rollback is a
  release, with a canary. A storage-migration rollback is not available at all.
- **On-call:** M2, M3, and M5 imply someone is paged outside business hours.
  Confirm that rota before launch, not after the first page.

## Related documents

| Document | Covers |
| --- | --- |
| [runbook-canary-upgrade.md](../../docs/runbook-canary-upgrade.md) | Upgrade and rollback mechanics |
| [scholarship-guide.md](scholarship-guide.md) | Contract behaviour, error catalogue, known gaps |
| [scholarship-staging.md](scholarship-staging.md) | Provisioning and verifying a test scenario |
| [ADR 0002](../../docs/adr/0002-scholarships-on-chain-boundaries.md) | Why the contracts are not linked |
