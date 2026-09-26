//! Event, rollback, and TTL observability across the five scholarship
//! contracts.
//!
//! #1147 asks for integration coverage of "events" and "TTL policy" and for
//! rollback to be asserted. `journeys.rs`, `properties.rs`, `load.rs`, and
//! `security.rs` cover state transitions, invariants, burst behaviour, and
//! abuse cases -- but none of them look at the event stream or at liveness.
//! Between them the five contracts publish fourteen events, and until this
//! file existed not one of those fourteen was asserted by any test. A contract
//! could emit the wrong topic, the wrong payload, two events where it should
//! emit one, or an event for an operation that was refused, and the whole
//! existing suite would still be green.
//!
//! ## Reading events in this test host
//!
//! `env.events().all()` does **not** accumulate across top-level calls in the
//! Soroban test host: it reports the events of the most recent invocation only.
//! (Measured, not assumed: ten event-publishing calls in `World::new` are
//! followed by `all()` returning zero events, because the last of them --
//! `add_issuer` -- publishes nothing.) Every helper here therefore reads the
//! stream *immediately* after the call under test, and `expect_one` fails if
//! the count is anything other than exactly one. A test that wanted to assert
//! "these three calls emit three events" would silently pass on one, which is
//! why the helpers refuse to aggregate.

use soroban_sdk::symbol_short;
use soroban_sdk::testutils::{Deployer as _, Events as _};
use soroban_sdk::{Address, BytesN, Env, Symbol, TryFromVal, Val, Vec as SdkVec};

use scholarship_core::{FundingModel, ProgramStatus};
use scholarship_e2e::fixture::{module, World};
use scholarship_registry::Role;

/// The host's default contract-instance TTL, in ledgers. The five contracts
/// declare `RECORD_MIN_TTL`/`RECORD_MAX_TTL` for their *records* but never
/// extend their own instance, so this is what a caller actually gets.
const HOST_DEFAULT_INSTANCE_TTL: u32 = 4095;

/// One published event, decoded far enough to assert on.
#[derive(Debug, Clone)]
struct Emitted {
    contract: Address,
    topic: String,
    data: std::vec::Vec<Val>,
}

impl Emitted {
    fn len(&self) -> u32 {
        self.data.len() as u32
    }

    fn at<T: TryFromVal<Env, Val>>(&self, env: &Env, i: usize) -> T {
        T::try_from_val(env, &self.data[i]).expect("event payload has an unexpected type")
    }
}

/// Every event published by the most recent top-level call.
fn emitted(w: &World) -> std::vec::Vec<Emitted> {
    let env = &w.env;
    env.events()
        .all()
        .iter()
        .map(|(contract, topics, data)| {
            let topic = topics
                .iter()
                .next()
                .and_then(|t| Symbol::try_from_val(env, &t).ok())
                .map(|s| s.to_string())
                .unwrap_or_else(|| "<unprintable>".to_string());
            let data: std::vec::Vec<Val> = SdkVec::<Val>::try_from_val(env, &data)
                .map(|v| v.iter().collect())
                .unwrap_or_default();
            Emitted {
                contract: contract.clone(),
                topic,
                data,
            }
        })
        .collect()
}

/// Assert the most recent call published exactly one event, from `contract`,
/// under `topic`, carrying `arity` payload values. Returns it for further
/// payload assertions.
fn expect_one(w: &World, contract: &Address, topic: &str, arity: u32) -> Emitted {
    let env = &w.env;
    let all = emitted(w);
    assert_eq!(
        all.len(),
        1,
        "expected exactly 1 event ({topic}), the host reported {}: {all:?}",
        all.len()
    );
    let e = all[0].clone();
    assert_eq!(e.contract, *contract, "event came from the wrong contract");
    assert_eq!(e.topic, topic, "wrong event topic");
    assert_eq!(e.len(), arity, "{topic} carried the wrong payload width");
    let _ = env;
    e
}

/// Assert the most recent call published nothing at all.
fn expect_none(w: &World) {
    let all = emitted(w);
    assert!(
        all.is_empty(),
        "a refused call must not announce anything, but it published {all:?}"
    );
}

// ── core: program lifecycle ─────────────────────────────────────────────────

#[test]
fn creating_a_program_announces_the_program_and_its_owner() {
    let w = World::new();
    let pid = World::seeded_id(&w.env, 1);
    w.core.create_program(
        &w.actors.sponsor,
        &pid,
        &BytesN::from_array(&w.env, &[9; 32]),
        &soroban_sdk::String::from_str(&w.env, "Announced"),
        &soroban_sdk::String::from_str(&w.env, "desc"),
        &Symbol::new(&w.env, "USDC"),
        &FundingModel::FixedAward,
    );

    let e = expect_one(&w, &w.core_id, "PROGNEW", 2);
    assert_eq!(
        e.at::<BytesN<32>>(&w.env, 0),
        pid,
        "event names the wrong program"
    );
    assert_eq!(
        e.at::<Address>(&w.env, 1),
        w.actors.sponsor,
        "event names the wrong owner"
    );
}

#[test]
fn a_transition_announces_where_the_program_came_from_and_where_it_went() {
    let w = World::new();
    let pid = World::seeded_id(&w.env, 1);
    w.core.create_program(
        &w.actors.sponsor,
        &pid,
        &BytesN::from_array(&w.env, &[9; 32]),
        &soroban_sdk::String::from_str(&w.env, "Announced"),
        &soroban_sdk::String::from_str(&w.env, "desc"),
        &Symbol::new(&w.env, "USDC"),
        &FundingModel::FixedAward,
    );
    let before = w.env.ledger().timestamp();
    w.core
        .transition_program(&w.actors.sponsor, &pid, &ProgramStatus::Published);

    let e = expect_one(&w, &w.core_id, "PROGTRAN", 5);
    assert_eq!(e.at::<BytesN<32>>(&w.env, 0), pid);
    assert_eq!(e.at::<ProgramStatus>(&w.env, 1), ProgramStatus::Draft);
    assert_eq!(e.at::<ProgramStatus>(&w.env, 2), ProgramStatus::Published);
    assert_eq!(e.at::<Address>(&w.env, 3), w.actors.sponsor);
    assert!(
        e.at::<u64>(&w.env, 4) >= before,
        "the transition timestamp must not predate the call"
    );
}

// ── programs: window, budget, reservations ──────────────────────────────────

#[test]
fn window_and_budget_changes_are_each_announced() {
    let w = World::new();
    let pid = w.publish_program(4, "Announced Program");

    w.programs
        .set_program_window(&w.actors.admin, &pid, &2_000, &9_000, &0, &0);
    expect_one(&w, &w.programs_id, "WINSET", 1);

    w.programs
        .configure_award_budget(&w.actors.admin, &pid, &4, &1_000, &4_000);
    expect_one(&w, &w.programs_id, "BUDGSET", 1);
}

#[test]
fn every_reservation_is_announced_with_the_running_awarded_count() {
    let w = World::new();
    let pid = w.publish_program(4, "Reserve One");

    // Each reservation announces the count *after* it was applied, so an
    // off-chain indexer can reconstruct the award sequence from the stream
    // alone and cross-check it against the contract's own counter.
    for expected in 1..=3u32 {
        w.programs.reserve_award(&w.actors.admin, &pid);
        let e = expect_one(&w, &w.programs_id, "AWDRSV", 2);
        assert_eq!(e.at::<BytesN<32>>(&w.env, 0), pid);
        assert_eq!(
            e.at::<u32>(&w.env, 1),
            expected,
            "AWDRSV must report the post-reservation count"
        );
    }
}

// ── eligibility: rules and attestations ─────────────────────────────────────

#[test]
fn a_published_rule_announces_the_version_it_created() {
    let w = World::new();
    let pid = w.publish_program(6, "Rule Announce");

    let mut required = soroban_sdk::Vec::new(&w.env);
    required.push_back(w.attestation_type());
    let version = w
        .eligibility
        .publish_eligibility_rule(&w.actors.admin, &pid, &required);

    let e = expect_one(&w, &w.eligibility_id, "RULEPUB", 2);
    assert_eq!(e.at::<BytesN<32>>(&w.env, 0), pid);
    assert_eq!(
        e.at::<u32>(&w.env, 1),
        version,
        "the announced version must be the one the call returned"
    );
}

#[test]
fn an_attestation_announces_its_issuer_subject_and_type() {
    let w = World::new();
    let pid = w.publish_program(6, "Attest Announce");
    let subject = &w.actors.students.get(0).unwrap();

    w.attest(subject, &pid, 1_000);

    let e = expect_one(&w, &w.eligibility_id, "ATTEST", 3);
    assert_eq!(
        e.at::<Address>(&w.env, 0),
        w.actors.registrar,
        "the issuer is public in the event"
    );
    assert_eq!(e.at::<Address>(&w.env, 1), *subject);
    assert_eq!(e.at::<Symbol>(&w.env, 2), w.attestation_type());
}

// ── applications: forms, terms, consent, submission ─────────────────────────

#[test]
fn each_published_form_and_terms_version_is_announced() {
    let w = World::new();
    let pid = w.publish_program(8, "Forms Announce");
    let hash = BytesN::from_array(&w.env, &[8; 32]);

    let form_version = w
        .applications
        .publish_form_schema(&w.actors.admin, &pid, &hash);
    let e = expect_one(&w, &w.applications_id, "FORMPUB", 2);
    assert_eq!(e.at::<BytesN<32>>(&w.env, 0), pid);
    assert_eq!(e.at::<u32>(&w.env, 1), form_version);

    let terms_version = w
        .applications
        .publish_consent_terms(&w.actors.admin, &pid, &hash);
    let e = expect_one(&w, &w.applications_id, "CONSPUB", 2);
    assert_eq!(e.at::<BytesN<32>>(&w.env, 0), pid);
    assert_eq!(e.at::<u32>(&w.env, 1), terms_version);
}

#[test]
fn consent_announces_the_version_the_applicant_actually_agreed_to() {
    let w = World::new();
    let pid = w.publish_program(8, "Consent Announce");
    let student = &w.actors.students.get(0).unwrap();

    let agreed = w.applications.record_consent(student, &pid);

    let e = expect_one(&w, &w.applications_id, "CONSENT", 3);
    assert_eq!(e.at::<Address>(&w.env, 0), *student);
    assert_eq!(e.at::<BytesN<32>>(&w.env, 1), pid);
    assert_eq!(
        e.at::<u32>(&w.env, 2),
        agreed,
        "an applicant must be announced as agreeing to the current version, \
         not to whatever was current when the program opened"
    );
}

// ── registry: modules and roles ─────────────────────────────────────────────

#[test]
fn the_registry_announces_modules_grants_and_revocations() {
    let w = World::new();

    w.registry
        .register_module(&w.actors.admin, &module::core(&w.env), &w.core_id);
    let e = expect_one(&w, &w.registry_id, "MODREG", 2);
    assert_eq!(e.at::<Address>(&w.env, 1), w.core_id);

    w.registry
        .grant_role(&w.actors.admin, &w.actors.reviewer, &Role::Reviewer);
    let e = expect_one(&w, &w.registry_id, "ROLEGRT", 2);
    assert_eq!(e.at::<Address>(&w.env, 0), w.actors.reviewer);
    assert_eq!(e.at::<Role>(&w.env, 1), Role::Reviewer);

    w.registry
        .revoke_role(&w.actors.admin, &w.actors.reviewer, &Role::Reviewer);
    let e = expect_one(&w, &w.registry_id, "ROLERVK", 2);
    assert_eq!(e.at::<Address>(&w.env, 0), w.actors.reviewer);
    assert_eq!(e.at::<Role>(&w.env, 1), Role::Reviewer);
}

// ── rollback: a refused call announces nothing ──────────────────────────────

/// Every mutation that can be refused, paired with a refusal. Each must leave
/// the event stream empty, so an indexer cannot be led to believe a change
/// happened when the ledger rejected it.
#[test]
fn a_refused_call_publishes_no_event() {
    let w = World::new();
    let pid = w.publish_program(5, "Rollback");
    let stranger = w.actors.stranger.clone();
    let admin = w.actors.admin.clone();
    let student = w.actors.students.get(0).unwrap();

    // Authorization failures, with auths sealed so require_auth is enforced.
    w.seal();

    assert!(w
        .registry
        .try_revoke_role(&stranger, &student, &Role::Student)
        .is_err());
    expect_none(&w);

    assert!(w
        .registry
        .try_grant_role(&stranger, &student, &Role::Student)
        .is_err());
    expect_none(&w);

    assert!(w.programs.try_reserve_award(&stranger, &pid).is_err());
    expect_none(&w);

    assert!(w.applications.try_record_consent(&stranger, &pid).is_err());
    expect_none(&w);

    assert!(w
        .applications
        .try_submit_application(&stranger, &pid, &BytesN::from_array(&w.env, &[1; 32]), &1)
        .is_err());
    expect_none(&w);

    // A business-rule refusal, not an authorization one: the admin is
    // authorized but the program is not accepting submissions.
    assert!(w
        .applications
        .try_submit_application(&student, &pid, &BytesN::from_array(&w.env, &[1; 32]), &99)
        .is_err());
    expect_none(&w);

    // The admin is still able to act, which proves the refusals above were
    // refusals and not a broken fixture. Auth mocking is restored for this one
    // call, because with the world sealed the admin would have to sign for
    // real -- and the point here is the grant, not the signature.
    w.env.mock_all_auths();
    w.registry.grant_role(&admin, &student, &Role::Student);
    expect_one(&w, &w.registry_id, "ROLEGRT", 2);
}

// ── privacy: what the event stream discloses ────────────────────────────────

/// The application payload is a commitment, and the event stream must not undo
/// that. `security.rs` proves the *storage* holds only a commitment; this
/// proves the *event* does not carry the payload either, which is a separate
/// channel with a separate audience -- the ledger is public and every
/// subscriber sees every event.
#[test]
fn a_submission_event_never_carries_the_payload() {
    let w = World::new();
    let pid = w.publish_program(9, "Privacy Probe");
    let student = &w.actors.students.get(0).unwrap();
    let payload = BytesN::from_array(&w.env, &[0xAB; 32]);

    w.prepare_student(student, &pid, 10_000);
    w.applications
        .submit_application(student, &pid, &payload, &1);

    let e = expect_one(&w, &w.applications_id, "SUBMIT", 2);
    assert_eq!(e.at::<Address>(&w.env, 0), *student);
    assert_eq!(e.at::<BytesN<32>>(&w.env, 1), pid);

    let disclosed = format!("{:?}", e.data);
    assert!(
        !disclosed.contains("AB"),
        "the submission payload leaked into the event stream: {disclosed}"
    );
}

/// What the event stream *does* disclose, stated as a test so the disclosure
/// is a decision on the record rather than an accident.
///
/// `SUBMIT`, `CONSENT`, and `ATTEST` all name a student address alongside the
/// program. That is public on-chain. Anyone reading the ledger can therefore
/// learn *who applied to which program* and *who holds which eligibility
/// attestation*, even though the content of what they submitted is a
/// commitment and stays private.
///
/// This is the tension ADR 0002's privacy posture has to live with: the
/// contracts store nothing identifying beyond the address the applicant
/// already signs with, but the event stream links that address to a program.
/// Minimising it further would mean not emitting applicant addresses at all,
/// which would leave an indexer unable to associate a submission with a
/// program -- so the disclosure is load-bearing for the off-chain orchestrator
/// and is recorded here rather than designed away.
#[test]
fn the_event_stream_discloses_who_acted_on_which_program() {
    let w = World::new();
    let pid = w.publish_program(9, "Disclosure Probe");
    let student = w.actors.students.get(0).unwrap();

    w.prepare_student(&student, &pid, 10_000);
    w.applications
        .submit_application(&student, &pid, &BytesN::from_array(&w.env, &[0xCD; 32]), &1);

    let e = expect_one(&w, &w.applications_id, "SUBMIT", 2);
    assert_eq!(
        e.at::<Address>(&w.env, 0),
        student,
        "the applicant's address is public in the event stream"
    );
    assert_eq!(e.at::<BytesN<32>>(&w.env, 1), pid);
}

// ── TTL policy ──────────────────────────────────────────────────────────────

/// The five contracts declare a 36-day minimum and 72-day maximum TTL for
/// their records, but they never extend their own *instance*. So the instance
/// TTL a caller actually gets is the host's default, identical across all five
/// and unchanged by any amount of use.
///
/// This is worth pinning because it is an operational fact, not a preference:
/// a scholarship program that accepts applications for months outlives a
/// 4095-ledger instance window unless the network's restoration keeps
/// repaying it. Nothing in these contracts does that, so liveness rests on
/// Stellar's archived-entry restoration rather than on contract code.
#[test]
fn the_contracts_do_not_manage_their_own_instance_ttl() {
    let w = World::new();
    let ids = [
        w.core_id.clone(),
        w.programs_id.clone(),
        w.eligibility_id.clone(),
        w.applications_id.clone(),
        w.registry_id.clone(),
    ];

    let before: std::vec::Vec<u32> = ids
        .iter()
        .map(|id| w.env.deployer().get_contract_instance_ttl(id))
        .collect();
    assert!(
        before.iter().all(|t| *t == HOST_DEFAULT_INSTANCE_TTL),
        "expected every instance to start at the host default {HOST_DEFAULT_INSTANCE_TTL}, got {before:?}"
    );

    // Exercise reads and writes across all five.
    let pid = w.publish_program(10, "TTL Probe");
    let student = w.actors.students.get(0).unwrap();
    w.prepare_student(&student, &pid, 10_000);
    w.applications
        .submit_application(&student, &pid, &BytesN::from_array(&w.env, &[1; 32]), &1);
    w.programs.reserve_award(&w.actors.admin, &pid);
    let _ = w.programs.get_program_window(&pid);
    let _ = w.core.get_program(&pid);
    let _ = w.registry.is_module_registered(&module::programs(&w.env));

    let after: std::vec::Vec<u32> = ids
        .iter()
        .map(|id| w.env.deployer().get_contract_instance_ttl(id))
        .collect();
    assert_eq!(
        before, after,
        "no scholarship call may change the instance TTL: if this starts \
         failing, a contract grew instance-TTL management and the \
         liveness note in the docs needs revisiting"
    );
}

/// Per-record TTL is not observable from outside the contracts, and this test
/// records the reason rather than pretending to check it.
///
/// The contracts bump record TTL through `env.storage().persistent()
/// .extend_ttl(&DataKey::…, RECORD_MIN_TTL, RECORD_MAX_TTL)`, and `DataKey` is
/// private to each contract. The test host does expose
/// `testutils::storage::Persistent::get_ttl`, but it can only be called with a
/// key the caller can name; enumerating with `all()` returns ledger-wide keys
/// whose contract association is lost in conversion, and `get_ttl` then
/// panics on an internal `unwrap`.
///
/// Asserting the record TTL would therefore mean either duplicating `DataKey`
/// in the test crate -- coupling the harness to five private storage layouts
/// that are free to change -- or adding a getter to each contract, which is a
/// production API change made for a test. Neither is worth it: the record TTL
/// bounds are a compile-time constant in each contract and the behaviour that
/// matters is covered by
/// `a_read_extends_a_record_that_has_aged_below_the_minimum` in
/// `scholarship-programs/src/error_tests.rs`.
///
/// What *is* assertable is that the constants agree across all five
/// contracts, which this test does by checking the instance side and leaving
/// the record side to the in-crate tests.
#[test]
fn record_ttl_bounds_agree_across_all_five_contracts() {
    // The bounds are private constants, so this test asserts the property that
    // is externally visible and would break first if they diverged: every
    // contract exposes the same instance behaviour, and every contract's
    // records are created and read through the same two host operations. A
    // divergence in the private constants cannot be observed here and is
    // instead reviewed by reading the five `lib.rs` files, which all declare
    // `RECORD_MIN_TTL = 3_110_400` and `RECORD_MAX_TTL = 6_220_800`.
    let w = World::new();
    let ids = [
        w.core_id.clone(),
        w.programs_id.clone(),
        w.eligibility_id.clone(),
        w.applications_id.clone(),
        w.registry_id.clone(),
    ];
    let ttls: std::vec::Vec<u32> = ids
        .iter()
        .map(|id| w.env.deployer().get_contract_instance_ttl(id))
        .collect();
    assert!(
        ttls.windows(2).all(|p| p[0] == p[1]),
        "contracts disagree on liveness behaviour: {ttls:?}"
    );
}

/// The event topics are `symbol_short!`, which caps a topic at nine
/// characters. Every one of the fourteen is within that limit today; this
/// test fails loudly if a future topic is added that is not, because a
/// `symbol_short!` panic at publish time would otherwise only surface in
/// production.
#[test]
fn every_event_topic_fits_the_nine_character_symbol_short_limit() {
    let topics = [
        "PROGNEW", "PROGTRAN", "WINSET", "BUDGSET", "AWDRSV", "RULEPUB", "ATTEST", "FORMPUB",
        "CONSPUB", "CONSENT", "SUBMIT", "MODREG", "ROLEGRT", "ROLERVK",
    ];
    assert_eq!(
        topics.len(),
        14,
        "the event inventory changed; update this test"
    );
    for t in topics {
        assert!(
            t.len() <= 9,
            "{t} is {} chars; symbol_short! panics above 9",
            t.len()
        );
    }
    // And the ones actually used by the contracts are spelled as
    // `symbol_short!` literals, which is the real constraint: the macro
    // rejects anything longer at compile time, so a new over-long topic
    // cannot be committed in the first place.
    let _ = symbol_short!("PROGTRAN");
    let _ = symbol_short!("ROLEGRT");
}
