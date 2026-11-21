#![cfg(test)]

extern crate std;

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    symbol_short, Address, BytesN, Env, Symbol,
};

use crate::{
    AmountOutcome, CapstoneEscrowConfirmationsContract,
    CapstoneEscrowConfirmationsContractClient, ConfirmFundingRequest, ConfirmationState,
    ContractError, EscrowConfirmation, Finality, ReasonCode, Reconciliation, Role,
};

fn hash(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn setup() -> (
    Env,
    Address,
    Address,
    Address,
    BytesN<32>,
    BytesN<32>,
    CapstoneEscrowConfirmationsContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CapstoneEscrowConfirmationsContract, ());
    let client = CapstoneEscrowConfirmationsContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let attestor = Address::generate(&env);
    let auditor = Address::generate(&env);
    let tenant = hash(&env, 20);
    let project = hash(&env, 21);
    client.initialize(&admin);
    client.grant_role(&admin, &tenant, &attestor, &Role::Attestor);
    client.grant_role(&admin, &tenant, &auditor, &Role::SettlementAuditor);
    (env, contract_id, admin, attestor, auditor, tenant, project, client)
}

fn confirm(
    client: &CapstoneEscrowConfirmationsContractClient,
    attestor: &Address,
    tenant: &BytesN<32>,
    project: &BytesN<32>,
    intent_id: &BytesN<32>,
    request_id: &BytesN<32>,
    intent_hash: &BytesN<32>,
    finality_hash: &BytesN<32>,
    expected_amount: i128,
    observed_amount: i128,
    asset: &BytesN<32>,
    destination_hash: &BytesN<32>,
    agreement_hash: &BytesN<32>,
    finality: Finality,
    amount_outcome: AmountOutcome,
    asset_outcome: Reconciliation,
    destination_outcome: Reconciliation,
    agreement_outcome: Reconciliation,
    expires_at: &u64,
    expected_version: u32,
) -> EscrowConfirmation {
    client.confirm_funding(
        attestor,
        tenant,
        project,
        &ConfirmFundingRequest {
            intent_id: intent_id.clone(),
            request_id: request_id.clone(),
            intent_hash: intent_hash.clone(),
            finality_hash: finality_hash.clone(),
            expected_amount,
            observed_amount,
            asset: asset.clone(),
            destination_hash: destination_hash.clone(),
            agreement_hash: agreement_hash.clone(),
            finality,
            amount_outcome,
            asset_outcome,
            destination_outcome,
            agreement_outcome,
            expires_at: *expires_at,
            expected_version,
        },
    )
}

fn confirm_error(
    client: &CapstoneEscrowConfirmationsContractClient,
    attestor: &Address,
    tenant: &BytesN<32>,
    project: &BytesN<32>,
    intent_id: &BytesN<32>,
    request_id: &BytesN<32>,
    intent_hash: &BytesN<32>,
    finality_hash: &BytesN<32>,
    expected_amount: i128,
    observed_amount: i128,
    asset: &BytesN<32>,
    destination_hash: &BytesN<32>,
    agreement_hash: &BytesN<32>,
    finality: Finality,
    amount_outcome: AmountOutcome,
    asset_outcome: Reconciliation,
    destination_outcome: Reconciliation,
    agreement_outcome: Reconciliation,
    expires_at: &u64,
    expected_version: u32,
) -> ContractError {
    client
        .try_confirm_funding(
            attestor,
            tenant,
            project,
            &ConfirmFundingRequest {
                intent_id: intent_id.clone(),
                request_id: request_id.clone(),
                intent_hash: intent_hash.clone(),
                finality_hash: finality_hash.clone(),
                expected_amount,
                observed_amount,
                asset: asset.clone(),
                destination_hash: destination_hash.clone(),
                agreement_hash: agreement_hash.clone(),
                finality,
                amount_outcome,
                asset_outcome,
                destination_outcome,
                agreement_outcome,
                expires_at: *expires_at,
                expected_version,
            },
        )
        .unwrap_err()
        .unwrap()
}

#[test]
fn confirmation_requires_stored_attestor_role_and_emits_public_values() {
    let (env, _, _, attestor, _, tenant, project, client) = setup();
    let outsider = Address::generate(&env);
    let expiry = env.ledger().timestamp() + 500;

    assert_eq!(
        confirm_error(
            &client,
            &outsider,
            &tenant,
            &project,
            &hash(&env, 30),
            &hash(&env, 31),
            &hash(&env, 32),
            &hash(&env, 33),
            1_000,
            1_000,
            &hash(&env, 34),
            &hash(&env, 35),
            &hash(&env, 36),
            Finality::Final,
            AmountOutcome::Exact,
            Reconciliation::Match,
            Reconciliation::Match,
            Reconciliation::Match,
            &expiry,
            0,
        ),
        ContractError::Unauthorized
    );

    let confirmation = confirm(
        &client,
        &attestor,
        &tenant,
        &project,
        &hash(&env, 30),
        &hash(&env, 31),
        &hash(&env, 32),
        &hash(&env, 33),
        1_000,
        1_000,
        &hash(&env, 34),
        &hash(&env, 35),
        &hash(&env, 36),
        Finality::Final,
        AmountOutcome::Exact,
        Reconciliation::Match,
        Reconciliation::Match,
        Reconciliation::Match,
        &expiry,
        0,
    );
    assert_eq!(confirmation.state, ConfirmationState::Confirmed);
    assert!(confirmation.funds_available);
    assert_eq!(confirmation.version, 1);
    assert_eq!(env.auths()[0].0, attestor);

    let events = env.events().all();
    assert_eq!(events.len(), 1);
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = topics.get(0).unwrap().try_into_val(&env).unwrap();
    assert_eq!(topic, symbol_short!("CBECCD"));
    let (event_tenant, event_project, event_intent, amount, asset):
        (BytesN<32>, BytesN<32>, BytesN<32>, i128, BytesN<32>) =
        data.try_into_val(&env).unwrap();
    assert_eq!(event_tenant, tenant);
    assert_eq!(event_project, project);
    assert_eq!(event_intent, hash(&env, 30));
    assert_eq!(amount, 1_000);
    assert_eq!(asset, hash(&env, 34));
}

#[test]
fn confirmation_retry_is_idempotent_and_binds_intent_hash() {
    let (env, _, _, attestor, _, tenant, project, client) = setup();
    let expiry = env.ledger().timestamp() + 500;

    let first = confirm(
        &client,
        &attestor,
        &tenant,
        &project,
        &hash(&env, 40),
        &hash(&env, 41),
        &hash(&env, 42),
        &hash(&env, 43),
        500,
        500,
        &hash(&env, 44),
        &hash(&env, 45),
        &hash(&env, 46),
        Finality::Final,
        AmountOutcome::Exact,
        Reconciliation::Match,
        Reconciliation::Match,
        Reconciliation::Match,
        &expiry,
        0,
    );
    let retry = confirm(
        &client,
        &attestor,
        &tenant,
        &project,
        &hash(&env, 40),
        &hash(&env, 41),
        &hash(&env, 42),
        &hash(&env, 43),
        500,
        500,
        &hash(&env, 44),
        &hash(&env, 45),
        &hash(&env, 46),
        Finality::Final,
        AmountOutcome::Exact,
        Reconciliation::Match,
        Reconciliation::Match,
        Reconciliation::Match,
        &expiry,
        0,
    );
    assert_eq!(retry, first);
    assert_eq!(env.events().all().len(), 0);

    assert_eq!(
        confirm_error(
            &client,
            &attestor,
            &tenant,
            &project,
            &hash(&env, 40),
            &hash(&env, 41),
            &hash(&env, 47),
            &hash(&env, 43),
            500,
            500,
            &hash(&env, 44),
            &hash(&env, 45),
            &hash(&env, 46),
            Finality::Final,
            AmountOutcome::Exact,
            Reconciliation::Match,
            Reconciliation::Match,
            Reconciliation::Match,
            &expiry,
            0,
        ),
        ContractError::RequestConflict
    );
    assert_eq!(
        confirm_error(
            &client,
            &attestor,
            &tenant,
            &project,
            &hash(&env, 40),
            &hash(&env, 48),
            &hash(&env, 42),
            &hash(&env, 43),
            500,
            500,
            &hash(&env, 44),
            &hash(&env, 45),
            &hash(&env, 46),
            Finality::Final,
            AmountOutcome::Exact,
            Reconciliation::Match,
            Reconciliation::Match,
            Reconciliation::Match,
            &expiry,
            0,
        ),
        ContractError::AlreadyConfirmed
    );
}

#[test]
fn finality_amount_asset_destination_agreement_and_expiry_are_reconciled() {
    let (env, _, _, attestor, _, tenant, project, client) = setup();
    let expiry = env.ledger().timestamp() + 500;

    assert_eq!(
        confirm_error(
            &client,
            &attestor,
            &tenant,
            &project,
            &hash(&env, 50),
            &hash(&env, 51),
            &hash(&env, 52),
            &hash(&env, 53),
            100,
            100,
            &hash(&env, 54),
            &hash(&env, 55),
            &hash(&env, 56),
            Finality::Pending,
            AmountOutcome::Exact,
            Reconciliation::Match,
            Reconciliation::Match,
            Reconciliation::Match,
            &expiry,
            0,
        ),
        ContractError::NotFinal
    );
    assert_eq!(
        confirm_error(
            &client,
            &attestor,
            &tenant,
            &project,
            &hash(&env, 57),
            &hash(&env, 58),
            &hash(&env, 59),
            &hash(&env, 60),
            100,
            90,
            &hash(&env, 54),
            &hash(&env, 55),
            &hash(&env, 56),
            Finality::Final,
            AmountOutcome::Exact,
            Reconciliation::Match,
            Reconciliation::Match,
            Reconciliation::Match,
            &expiry,
            0,
        ),
        ContractError::Underpayment
    );
    assert_eq!(
        confirm_error(
            &client,
            &attestor,
            &tenant,
            &project,
            &hash(&env, 61),
            &hash(&env, 62),
            &hash(&env, 63),
            &hash(&env, 64),
            100,
            110,
            &hash(&env, 54),
            &hash(&env, 55),
            &hash(&env, 56),
            Finality::Final,
            AmountOutcome::Exact,
            Reconciliation::Match,
            Reconciliation::Match,
            Reconciliation::Match,
            &expiry,
            0,
        ),
        ContractError::Overpayment
    );
    assert_eq!(
        confirm_error(
            &client,
            &attestor,
            &tenant,
            &project,
            &hash(&env, 65),
            &hash(&env, 66),
            &hash(&env, 67),
            &hash(&env, 68),
            100,
            100,
            &hash(&env, 54),
            &hash(&env, 55),
            &hash(&env, 56),
            Finality::Final,
            AmountOutcome::Exact,
            Reconciliation::Mismatch,
            Reconciliation::Match,
            Reconciliation::Match,
            &expiry,
            0,
        ),
        ContractError::AssetMismatch
    );
    assert_eq!(
        confirm_error(
            &client,
            &attestor,
            &tenant,
            &project,
            &hash(&env, 69),
            &hash(&env, 70),
            &hash(&env, 71),
            &hash(&env, 72),
            100,
            100,
            &hash(&env, 54),
            &hash(&env, 55),
            &hash(&env, 56),
            Finality::Final,
            AmountOutcome::Exact,
            Reconciliation::Match,
            Reconciliation::Mismatch,
            Reconciliation::Match,
            &expiry,
            0,
        ),
        ContractError::DestinationMismatch
    );
    assert_eq!(
        confirm_error(
            &client,
            &attestor,
            &tenant,
            &project,
            &hash(&env, 73),
            &hash(&env, 74),
            &hash(&env, 75),
            &hash(&env, 76),
            100,
            100,
            &hash(&env, 54),
            &hash(&env, 55),
            &hash(&env, 56),
            Finality::Final,
            AmountOutcome::Exact,
            Reconciliation::Match,
            Reconciliation::Match,
            Reconciliation::Mismatch,
            &expiry,
            0,
        ),
        ContractError::AgreementMismatch
    );
    assert_eq!(
        confirm_error(
            &client,
            &attestor,
            &tenant,
            &project,
            &hash(&env, 77),
            &hash(&env, 78),
            &hash(&env, 79),
            &hash(&env, 80),
            100,
            100,
            &hash(&env, 54),
            &hash(&env, 55),
            &hash(&env, 56),
            Finality::Final,
            AmountOutcome::Under,
            Reconciliation::Match,
            Reconciliation::Match,
            Reconciliation::Match,
            &expiry,
            0,
        ),
        ContractError::InvalidOutcome
    );
    assert_eq!(
        confirm_error(
            &client,
            &attestor,
            &tenant,
            &project,
            &hash(&env, 81),
            &hash(&env, 82),
            &hash(&env, 83),
            &hash(&env, 84),
            100,
            100,
            &hash(&env, 54),
            &hash(&env, 55),
            &hash(&env, 56),
            Finality::Final,
            AmountOutcome::Exact,
            Reconciliation::Match,
            Reconciliation::Match,
            Reconciliation::Match,
            &env.ledger().timestamp(),
            0,
        ),
        ContractError::Expired
    );
}

#[test]
fn rejection_is_idempotent_and_excludes_confirmation() {
    let (env, _, _, attestor, auditor, tenant, project, client) = setup();
    let intent_id = hash(&env, 90);
    let request_id = hash(&env, 91);
    let intent_hash = hash(&env, 92);

    let rejection = client.reject_funding(
        &auditor,
        &tenant,
        &project,
        &intent_id,
        &request_id,
        &intent_hash,
        &ReasonCode::Underpayment,
        &0u32,
    );
    assert_eq!(rejection.state, ConfirmationState::Rejected);
    assert_eq!(rejection.reason, ReasonCode::Underpayment);
    let events = env.events().all();
    assert_eq!(events.len(), 1);
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = topics.get(0).unwrap().try_into_val(&env).unwrap();
    assert_eq!(topic, symbol_short!("CBEREJ"));
    let (event_tenant, event_project, event_intent, reason):
        (BytesN<32>, BytesN<32>, BytesN<32>, ReasonCode) = data.try_into_val(&env).unwrap();
    assert_eq!(event_tenant, tenant);
    assert_eq!(event_project, project);
    assert_eq!(event_intent, intent_id);
    assert_eq!(reason, ReasonCode::Underpayment);

    assert_eq!(
        client.reject_funding(
            &auditor,
            &tenant,
            &project,
            &intent_id,
            &request_id,
            &intent_hash,
            &ReasonCode::Underpayment,
            &0u32,
        ),
        rejection
    );
    assert_eq!(env.events().all().len(), 0);
    assert_eq!(
        client.try_reject_funding(
            &auditor,
            &tenant,
            &project,
            &intent_id,
            &hash(&env, 93),
            &intent_hash,
            &ReasonCode::Overpayment,
            &0u32,
        ),
        Err(Ok(ContractError::AlreadyRejected))
    );
    assert_eq!(
        client.try_reject_funding(
            &attestor,
            &tenant,
            &project,
            &hash(&env, 94),
            &hash(&env, 95),
            &hash(&env, 96),
            &ReasonCode::NotFinal,
            &0u32,
        ),
        Err(Ok(ContractError::Unauthorized))
    );

    let expiry = env.ledger().timestamp() + 500;
    assert_eq!(
        confirm_error(
            &client,
            &attestor,
            &tenant,
            &project,
            &intent_id,
            &hash(&env, 97),
            &intent_hash,
            &hash(&env, 98),
            100,
            100,
            &hash(&env, 99),
            &hash(&env, 100),
            &hash(&env, 101),
            Finality::Final,
            AmountOutcome::Exact,
            Reconciliation::Match,
            Reconciliation::Match,
            Reconciliation::Match,
            &expiry,
            0,
        ),
        ContractError::InvalidState
    );
}

#[test]
fn index_bound_and_record_readback_are_deterministic() {
    let (env, contract_id, _, attestor, _, tenant, project, client) = setup();
    let expiry = env.ledger().timestamp() + 500;
    let mut seed = 110u8;
    for _ in 0..16 {
        confirm(
            &client,
            &attestor,
            &tenant,
            &project,
            &hash(&env, seed),
            &hash(&env, seed + 1),
            &hash(&env, seed + 2),
            &hash(&env, seed + 3),
            100,
            100,
            &hash(&env, seed + 4),
            &hash(&env, seed + 5),
            &hash(&env, seed + 6),
            Finality::Final,
            AmountOutcome::Exact,
            Reconciliation::Match,
            Reconciliation::Match,
            Reconciliation::Match,
            &expiry,
            0,
        );
        seed += 8;
    }
    assert_eq!(
        confirm_error(
            &client,
            &attestor,
            &tenant,
            &project,
            &hash(&env, 240),
            &hash(&env, 241),
            &hash(&env, 242),
            &hash(&env, 243),
            100,
            100,
            &hash(&env, 244),
            &hash(&env, 245),
            &hash(&env, 246),
            Finality::Final,
            AmountOutcome::Exact,
            Reconciliation::Match,
            Reconciliation::Match,
            Reconciliation::Match,
            &expiry,
            0,
        ),
        ContractError::CapacityExceeded
    );

    let fresh = CapstoneEscrowConfirmationsContractClient::new(&env, &contract_id);
    assert_eq!(
        fresh.get_confirmation(&tenant, &project, &hash(&env, 110)),
        client.get_confirmation(&tenant, &project, &hash(&env, 110))
    );
}
