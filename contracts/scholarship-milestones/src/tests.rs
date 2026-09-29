use crate::{
    ContractError, MilestoneInput, MilestoneKind, ScholarshipMilestonesContract,
    ScholarshipMilestonesContractClient,
};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, Symbol, Vec};

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ScholarshipMilestonesContract, ());
    let admin = Address::generate(&env);
    let client = ScholarshipMilestonesContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    (env, contract_id, admin)
}

fn client<'a>(env: &'a Env, contract_id: &Address) -> ScholarshipMilestonesContractClient<'a> {
    ScholarshipMilestonesContractClient::new(env, contract_id)
}

/// Build a `Vec<MilestoneInput>` from `(kind, bps, due_at)` triples.
fn inputs(env: &Env, specs: &[(MilestoneKind, u32, u64)]) -> Vec<MilestoneInput> {
    let mut v = Vec::new(env);
    for (kind, bps, due) in specs.iter() {
        v.push_back(MilestoneInput {
            kind: *kind,
            label_hash: hash(env, *bps as u8),
            percentage_bps: *bps,
            due_at: *due,
        });
    }
    v
}

/// 40/30/20/10 = 100% over ordered dates.
fn standard(env: &Env) -> Vec<MilestoneInput> {
    inputs(
        env,
        &[
            (MilestoneKind::Enrollment, 4_000, 100),
            (MilestoneKind::Attendance, 3_000, 200),
            (MilestoneKind::Coursework, 2_000, 300),
            (MilestoneKind::Completion, 1_000, 400),
        ],
    )
}

// ── initialization / authorization ─────────────────────────────────────────

#[test]
fn test_initialize_twice_fails() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    assert_eq!(
        c.try_initialize(&admin),
        Err(Ok(ContractError::AlreadyInitialized))
    );
}

#[test]
fn test_non_admin_cannot_define() {
    let (env, contract_id, _admin) = setup();
    let c = client(&env, &contract_id);
    let attacker = Address::generate(&env);
    let result = c.try_define_schedule(
        &attacker,
        &hash(&env, 1),
        &10_000i128,
        &Symbol::new(&env, "USD"),
        &standard(&env),
    );
    assert_eq!(result, Err(Ok(ContractError::NotAdmin)));
}

// ── #1093 — define: percentages and amounts reconcile ──────────────────────

#[test]
fn test_define_derives_amounts_that_reconcile() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    c.define_schedule(
        &admin,
        &hash(&env, 1),
        &10_000i128,
        &Symbol::new(&env, "USD"),
        &standard(&env),
    );

    let schedule = c.get_schedule(&hash(&env, 1));
    assert_eq!(schedule.total_amount, 10_000);
    assert_eq!(schedule.version, 1);
    assert!(!schedule.active);
    assert_eq!(schedule.milestones.len(), 4);

    let m0 = c.get_milestone(&hash(&env, 1), &0);
    let m1 = c.get_milestone(&hash(&env, 1), &1);
    let m2 = c.get_milestone(&hash(&env, 1), &2);
    let m3 = c.get_milestone(&hash(&env, 1), &3);
    assert_eq!(m0.amount, 4_000);
    assert_eq!(m1.amount, 3_000);
    assert_eq!(m2.amount, 2_000);
    assert_eq!(m3.amount, 1_000);
    assert_eq!(m0.kind, MilestoneKind::Enrollment);
    assert_eq!(m3.kind, MilestoneKind::Completion);
}

#[test]
fn test_amounts_sum_exactly_with_rounding_remainder() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    // 33.33% / 33.33% / 33.34% of 1_000 — integer division would otherwise
    // lose a unit; the last milestone absorbs the remainder.
    let v = inputs(
        &env,
        &[
            (MilestoneKind::Enrollment, 3_333, 100),
            (MilestoneKind::Coursework, 3_333, 200),
            (MilestoneKind::Completion, 3_334, 300),
        ],
    );
    c.define_schedule(
        &admin,
        &hash(&env, 1),
        &1_000i128,
        &Symbol::new(&env, "USD"),
        &v,
    );

    let m0 = c.get_milestone(&hash(&env, 1), &0);
    let m1 = c.get_milestone(&hash(&env, 1), &1);
    let m2 = c.get_milestone(&hash(&env, 1), &2);
    assert_eq!(m0.amount, 333);
    assert_eq!(m1.amount, 333);
    assert_eq!(m2.amount, 334);
    assert_eq!(m0.amount + m1.amount + m2.amount, 1_000);
}

#[test]
fn test_percentages_must_sum_to_100() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let v = inputs(
        &env,
        &[
            (MilestoneKind::Enrollment, 4_000, 100),
            (MilestoneKind::Completion, 5_000, 200),
        ],
    );
    let result = c.try_define_schedule(
        &admin,
        &hash(&env, 1),
        &10_000i128,
        &Symbol::new(&env, "USD"),
        &v,
    );
    assert_eq!(result, Err(Ok(ContractError::PercentagesDoNotSumTo100)));
}

#[test]
fn test_dates_must_be_strictly_ordered() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let v = inputs(
        &env,
        &[
            (MilestoneKind::Enrollment, 5_000, 300),
            (MilestoneKind::Completion, 5_000, 200), // earlier than previous
        ],
    );
    let result = c.try_define_schedule(
        &admin,
        &hash(&env, 1),
        &10_000i128,
        &Symbol::new(&env, "USD"),
        &v,
    );
    assert_eq!(result, Err(Ok(ContractError::DatesNotOrdered)));
}

#[test]
fn test_milestone_count_bounds() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let empty: Vec<MilestoneInput> = Vec::new(&env);
    let result = c.try_define_schedule(
        &admin,
        &hash(&env, 1),
        &10_000i128,
        &Symbol::new(&env, "USD"),
        &empty,
    );
    assert_eq!(result, Err(Ok(ContractError::InvalidMilestoneCount)));
}

#[test]
fn test_non_positive_total_rejected() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let result = c.try_define_schedule(
        &admin,
        &hash(&env, 1),
        &0i128,
        &Symbol::new(&env, "USD"),
        &standard(&env),
    );
    assert_eq!(result, Err(Ok(ContractError::InvalidAmount)));
}

#[test]
fn test_duplicate_define_rejected() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    c.define_schedule(
        &admin,
        &hash(&env, 1),
        &10_000i128,
        &Symbol::new(&env, "USD"),
        &standard(&env),
    );
    let result = c.try_define_schedule(
        &admin,
        &hash(&env, 1),
        &10_000i128,
        &Symbol::new(&env, "USD"),
        &standard(&env),
    );
    assert_eq!(result, Err(Ok(ContractError::ScheduleAlreadyExists)));
}

// ── #1093 — activation and governed amendment ──────────────────────────────

#[test]
fn test_activate_once() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    c.define_schedule(
        &admin,
        &hash(&env, 1),
        &10_000i128,
        &Symbol::new(&env, "USD"),
        &standard(&env),
    );
    c.activate_schedule(&admin, &hash(&env, 1));
    assert!(c.get_schedule(&hash(&env, 1)).active);

    let result = c.try_activate_schedule(&admin, &hash(&env, 1));
    assert_eq!(result, Err(Ok(ContractError::ScheduleAlreadyActive)));
}

#[test]
fn test_amend_requires_active_schedule() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    c.define_schedule(
        &admin,
        &hash(&env, 1),
        &10_000i128,
        &Symbol::new(&env, "USD"),
        &standard(&env),
    );
    let result = c.try_amend_schedule(
        &admin,
        &hash(&env, 1),
        &10_000i128,
        &Symbol::new(&env, "USD"),
        &standard(&env),
    );
    assert_eq!(result, Err(Ok(ContractError::ScheduleInactive)));
}

#[test]
fn test_amend_bumps_version_and_new_amounts() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    c.define_schedule(
        &admin,
        &hash(&env, 1),
        &10_000i128,
        &Symbol::new(&env, "USD"),
        &standard(&env),
    );
    c.activate_schedule(&admin, &hash(&env, 1));

    let amended = inputs(
        &env,
        &[
            (MilestoneKind::Enrollment, 5_000, 100),
            (MilestoneKind::Completion, 5_000, 200),
        ],
    );
    c.amend_schedule(
        &admin,
        &hash(&env, 1),
        &20_000i128,
        &Symbol::new(&env, "USD"),
        &amended,
    );

    let schedule = c.get_schedule(&hash(&env, 1));
    assert_eq!(schedule.version, 2);
    assert_eq!(schedule.total_amount, 20_000);
    assert_eq!(schedule.milestones.len(), 2);
    assert_eq!(c.get_milestone(&hash(&env, 1), &0).amount, 10_000);
    assert_eq!(c.get_milestone(&hash(&env, 1), &1).amount, 10_000);
}

#[test]
fn test_amend_locked_after_release() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    c.define_schedule(
        &admin,
        &hash(&env, 1),
        &10_000i128,
        &Symbol::new(&env, "USD"),
        &standard(&env),
    );
    c.activate_schedule(&admin, &hash(&env, 1));
    c.verify_milestone(&admin, &hash(&env, 1), &0);
    c.release_milestone(&admin, &hash(&env, 1), &0);

    let result = c.try_amend_schedule(
        &admin,
        &hash(&env, 1),
        &20_000i128,
        &Symbol::new(&env, "USD"),
        &standard(&env),
    );
    assert_eq!(
        result,
        Err(Ok(ContractError::ScheduleLockedAfterDisbursement))
    );
}

// ── #1093 — verification and ordered release ──────────────────────────────

#[test]
fn test_verify_requires_active_schedule() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    c.define_schedule(
        &admin,
        &hash(&env, 1),
        &10_000i128,
        &Symbol::new(&env, "USD"),
        &standard(&env),
    );
    let result = c.try_verify_milestone(&admin, &hash(&env, 1), &0);
    assert_eq!(result, Err(Ok(ContractError::ScheduleInactive)));
}

#[test]
fn test_cannot_verify_twice() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    c.define_schedule(
        &admin,
        &hash(&env, 1),
        &10_000i128,
        &Symbol::new(&env, "USD"),
        &standard(&env),
    );
    c.activate_schedule(&admin, &hash(&env, 1));
    c.verify_milestone(&admin, &hash(&env, 1), &0);

    let result = c.try_verify_milestone(&admin, &hash(&env, 1), &0);
    assert_eq!(result, Err(Ok(ContractError::MilestoneAlreadyVerified)));
}

#[test]
fn test_unverified_milestone_cannot_be_released() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    c.define_schedule(
        &admin,
        &hash(&env, 1),
        &10_000i128,
        &Symbol::new(&env, "USD"),
        &standard(&env),
    );
    c.activate_schedule(&admin, &hash(&env, 1));

    let result = c.try_release_milestone(&admin, &hash(&env, 1), &0);
    assert_eq!(result, Err(Ok(ContractError::MilestoneNotVerified)));
}

#[test]
fn test_release_must_be_in_order() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    c.define_schedule(
        &admin,
        &hash(&env, 1),
        &10_000i128,
        &Symbol::new(&env, "USD"),
        &standard(&env),
    );
    c.activate_schedule(&admin, &hash(&env, 1));
    c.verify_milestone(&admin, &hash(&env, 1), &1);

    let result = c.try_release_milestone(&admin, &hash(&env, 1), &1);
    assert_eq!(result, Err(Ok(ContractError::OutOfOrderRelease)));
}

#[test]
fn test_release_flow_tracks_released_and_remaining() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    c.define_schedule(
        &admin,
        &hash(&env, 1),
        &10_000i128,
        &Symbol::new(&env, "USD"),
        &standard(&env),
    );
    c.activate_schedule(&admin, &hash(&env, 1));

    c.verify_milestone(&admin, &hash(&env, 1), &0);
    c.release_milestone(&admin, &hash(&env, 1), &0);
    assert_eq!(c.released_total(&hash(&env, 1)), 4_000);
    assert_eq!(c.remaining_amount(&hash(&env, 1)), 6_000);

    let duplicate = c.try_release_milestone(&admin, &hash(&env, 1), &0);
    assert_eq!(duplicate, Err(Ok(ContractError::MilestoneAlreadyReleased)));

    c.verify_milestone(&admin, &hash(&env, 1), &1);
    c.release_milestone(&admin, &hash(&env, 1), &1);
    assert_eq!(c.released_total(&hash(&env, 1)), 7_000);
    assert_eq!(c.remaining_amount(&hash(&env, 1)), 3_000);
}

#[test]
fn test_index_out_of_range() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    c.define_schedule(
        &admin,
        &hash(&env, 1),
        &10_000i128,
        &Symbol::new(&env, "USD"),
        &standard(&env),
    );
    let result = c.try_get_milestone(&hash(&env, 1), &99);
    assert_eq!(result, Err(Ok(ContractError::MilestoneIndexOutOfRange)));
}

#[test]
fn test_schedule_not_found() {
    let (env, contract_id, _admin) = setup();
    let c = client(&env, &contract_id);
    let result = c.try_get_schedule(&hash(&env, 42));
    assert_eq!(result, Err(Ok(ContractError::ScheduleNotFound)));
}

#[test]
fn test_custom_milestone_keeps_label_commitment() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let mut v: Vec<MilestoneInput> = Vec::new(&env);
    v.push_back(MilestoneInput {
        kind: MilestoneKind::Custom,
        label_hash: hash(&env, 77),
        percentage_bps: 10_000,
        due_at: 500,
    });
    c.define_schedule(
        &admin,
        &hash(&env, 1),
        &1_000i128,
        &Symbol::new(&env, "USD"),
        &v,
    );

    let m = c.get_milestone(&hash(&env, 1), &0);
    assert_eq!(m.kind, MilestoneKind::Custom);
    assert_eq!(m.label_hash, hash(&env, 77));
}

#[test]
fn test_version() {
    let (env, contract_id, _admin) = setup();
    let c = client(&env, &contract_id);
    assert_eq!(c.version(), 1);
}
