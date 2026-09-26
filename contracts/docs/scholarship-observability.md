# Scholarship observability: events, rollback, and TTL

Implements issue [#1147](https://github.com/Gen-x-academy/chainVerse-onchain/issues/1147)
(integration tests) on the stacked branch that also carries
[#1146](https://github.com/Gen-x-academy/chainVerse-onchain/issues/1146).
See [`scholarship-test-coverage.md`](scholarship-test-coverage.md) for why the
branch is stacked on #1174.

## What was already covered, and what was not

`scholarship-e2e` already carried 37 tests: full journeys, generated property
sequences, burst load at a deadline, and adversarial cases. All of them assert
on **state**. None of them look at the event stream or at liveness.

That left two whole acceptance criteria from #1147 unevidenced:

| #1147 asks for | Before | Now |
| --- | --- | --- |
| events | 0 of 14 events asserted | 14 of 14 |
| TTL policy | no assertion of any kind | instance liveness pinned; record bounds documented |
| rollback / idempotency | replay covered in `security.rs` | every refused mutation proven to publish nothing |
| authorization | 14 abuse cases | unchanged, plus event-stream disclosure |

A contract could have emitted the wrong topic, the wrong payload, two events
where it should emit one, or an event for an operation the ledger rejected, and
all 37 tests would still have passed.

## Reading events in the Soroban test host

`env.events().all()` does **not** accumulate across top-level calls. It reports
the events of the most recent invocation only.

This was measured, not assumed, and it is the kind of thing that produces a
test suite which quietly asserts nothing. `World::new()` performs ten
event-publishing calls — five `register_module` and five `grant_role` — and
`all()` immediately afterwards returns **zero** events, because the last call it
makes is `add_issuer`, which publishes nothing. Two further calls bring the
count to one, not three.

So every helper in `observability.rs` reads the stream *immediately* after the
call under test, and `expect_one` fails unless the count is exactly one. A
helper that aggregated across calls would report one event for three calls and
pass; the tests are built so that mistake cannot pass silently.

## The event inventory

Fourteen events across five contracts. Each is asserted for topic, payload
width, payload contents, and source contract.

| Contract | Topic | Payload | Asserted by |
| --- | --- | --- | --- |
| `core` | `PROGNEW` | `(program_id, owner)` | `creating_a_program_announces_the_program_and_its_owner` |
| `core` | `PROGTRAN` | `(program_id, from, to, caller, now)` | `a_transition_announces_where_the_program_came_from_and_where_it_went` |
| `programs` | `WINSET` | `(program_id)` | `window_and_budget_changes_are_each_announced` |
| `programs` | `BUDGSET` | `(program_id)` | `window_and_budget_changes_are_each_announced` |
| `programs` | `AWDRSV` | `(program_id, awarded_count)` | `every_reservation_is_announced_with_the_running_awarded_count` |
| `eligibility` | `RULEPUB` | `(program_id, version)` | `a_published_rule_announces_the_version_it_created` |
| `eligibility` | `ATTEST` | `(issuer, subject, type)` | `an_attestation_announces_its_issuer_subject_and_type` |
| `applications` | `FORMPUB` | `(program_id, version)` | `each_published_form_and_terms_version_is_announced` |
| `applications` | `CONSPUB` | `(program_id, version)` | `each_published_form_and_terms_version_is_announced` |
| `applications` | `CONSENT` | `(applicant, program_id, version)` | `consent_announces_the_version_the_applicant_actually_agreed_to` |
| `applications` | `SUBMIT` | `(applicant, program_id)` | `a_submission_event_never_carries_the_payload` |
| `registry` | `MODREG` | `(name, address)` | `the_registry_announces_modules_grants_and_revocations` |
| `registry` | `ROLEGRT` | `(account, role)` | `the_registry_announces_modules_grants_and_revocations` |
| `registry` | `ROLERVK` | `(account, role)` | `the_registry_announces_modules_grants_and_revocations` |

Two of these carry a version that the *call also returns*, and the tests assert
the two agree. That is not redundant: an off-chain indexer that trusts the event
stream rather than the transaction result needs the two to be consistent, and
`CONSENT` in particular must announce the version the applicant agreed to, not
the one current when the program opened. A re-published consent-terms version
between reading and submitting is exactly the case
`republished_terms_strand_applicants_who_must_re_consent` covers on the state
side.

`AWDRSV` reports the count *after* the reservation is applied, so the event
stream alone reconstructs the award sequence and can be cross-checked against
the contract's own counter.

## Rollback: a refused call announces nothing

`a_refused_call_publishes_no_event` walks six refusals — five authorization
failures under a sealed world, and one business-rule refusal from an authorized
admin — and asserts the event stream is empty after each. The suite then proves
the refusals were refusals and not a broken fixture by having the admin grant a
role successfully afterwards.

This matters for indexers. An event emitted by a call the ledger rejected would
put an off-chain system permanently out of step with on-chain state, and no
state assertion anywhere else in the suite would catch it.

## Privacy: what the stream does and does not disclose

`security.rs` already proves the *storage* holds only a commitment for a
submission. The event stream is a separate channel with a wider audience — the
ledger is public and every subscriber sees every event — so it needed its own
check.

**The payload does not leak.** `a_submission_event_never_carries_the_payload`
asserts `SUBMIT` carries exactly two values and that neither is the submitted
`data_hash`.

**Participation does leak, and that is recorded as a decision.**
`the_event_stream_discloses_who_acted_on_which_program` asserts the opposite:
`SUBMIT` names the applicant's address alongside the program, `CONSENT` does the
same, and `ATTEST` names issuer, subject, and attestation type.

So anyone reading the ledger can learn *who applied to which program* and *who
holds which eligibility attestation*, even though the content of what they
submitted stays private. Storage is minimized to the address the applicant
already signs with; the event stream then links that address to a program.

This is worth stating plainly rather than designing away, because removing it
is not free: an indexer that cannot associate a submission with a program cannot
build a candidate's award status, which is the entire point of the platform. The
disclosure is load-bearing for the off-chain orchestrator, and the test exists so
that it stays a decision on the record. If the epic later decides the disclosure
is unacceptable, this test is where that change should start.

## TTL policy

All five contracts declare `RECORD_MIN_TTL = 3_110_400` (~36 days) and
`RECORD_MAX_TTL = 6_220_800` (~72 days), and bump record TTL on read. Two
findings, one asserted and one documented.

**Asserted — the contracts do not manage their own instance TTL.**
`the_contracts_do_not_manage_their_own_instance_ttl` checks that all five
instances sit at the host default (4095 ledgers) and that no scholarship call
changes that, across reads and writes on every contract.

This is an operational fact, not a preference. A program accepting applications
over months outlives a 4095-ledger instance window, and nothing in these
contracts repays it. Ageing the test ledger past the record window archives the
**contract instance** — reads then fail with the instance key archived, not with
a missing record — so liveness rests on Stellar's archived-entry restoration
rather than on contract code. An operator who assumes a stored record implies a
callable contract will be wrong.

**Documented — per-record TTL is not observable from an integration test.**
`DataKey` is private to each contract, and the test host's
`testutils::storage::Persistent::get_ttl` needs a nameable key. Enumerating with
`all()` returns ledger-wide keys whose contract association is lost in
conversion, and `get_ttl` then panics on an internal `unwrap` — verified, not
assumed. Asserting record TTL would mean either duplicating five private storage
layouts in the harness, or adding a getter to each contract, which is a
production API change made for a test. The record-side behaviour is covered
in-crate instead, where the keys are reachable, by
`scholarship-programs/src/error_tests.rs`.

`record_ttl_bounds_agree_across_all_five_contracts` pins the externally visible
half of that, and the constants themselves are reviewed by reading the five
`lib.rs` files.

## Do these tests actually bite?

Two deliberate mutations, each reverted after confirming the failure:

| Mutation | Result |
| --- | --- |
| `AWDRSV` → `AWDRSVX` | `every_reservation_is_announced_with_the_running_awarded_count` fails on `wrong event topic` |
| `SUBMIT` payload gains `data_hash` | both `a_submission_event_never_carries_the_payload` and `the_event_stream_discloses_who_acted_on_which_program` fail on `wrong payload width` |

## Results

52 tests in `scholarship-e2e`, up from 37.

| Suite | Tests | Covers |
| --- | --- | --- |
| `journeys.rs` | 11 | #1148 |
| `properties.rs` | 5 | #1149 |
| `load.rs` | 7 | #1150 |
| `security.rs` | 14 | #1151 |
| `observability.rs` | 15 | #1147 |

Clippy clean under `-D warnings`; `rustfmt` clean.

## Ownership, privacy, migration, operational impact

**Production code changed: none.** This adds one test file and one paragraph to
the crate docs. No contract logic, storage layout, ABI, or authorization is
touched, so migration impact is nil and there is nothing to coordinate with a
deployment.

**Ownership:** unchanged. The suite only reads events and TTLs; it writes
nothing outside its own `World`.

**Privacy:** the suite is the artifact that documents the applicant-disclosure
tension described above. It adds no new disclosure — the events it asserts were
already published — it makes an existing one explicit and testable.

**Operational impact:** two notes for whoever runs this in production, both
recorded as tests above. The event stream is now a contract that indexers can
rely on, so changing a topic or payload is a breaking change for off-chain
subscribers and should be treated as one. And contract-instance liveness is not
managed by contract code, so a deployment needs either periodic restoration or
an understanding that Stellar repays the instance on access.
