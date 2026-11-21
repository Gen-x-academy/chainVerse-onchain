use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env, Vec,
};

use crate::{
    CapstoneFundingModelsContract, CapstoneFundingModelsContractClient, ContractError,
    FundingModelKind, RecipientShare, Role,
};

fn setup() -> (
    Env,
    Address,
    Address,
    CapstoneFundingModelsContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(77);
    let id = env.register(CapstoneFundingModelsContract, ());
    let client = CapstoneFundingModelsContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let configurer = Address::generate(&env);
    client.initialize(&admin);
    let tenant = BytesN::from_array(&env, &[1u8; 32]);
    let project = BytesN::from_array(&env, &[2u8; 32]);
    client.set_role(&admin, &configurer, &tenant, &project, &Role::Configurer, &true);
    (env, admin, configurer, client)
}

#[test]
fn configures_funded_model_idempotently_and_validates_shares() {
    let (env, _admin, configurer, client) = setup();
    let tenant = BytesN::from_array(&env, &[1u8; 32]);
    let project = BytesN::from_array(&env, &[2u8; 32]);
    let intent = BytesN::from_array(&env, &[3u8; 32]);
    let asset = Address::generate(&env);
    let payer = Some(Address::generate(&env));
    let recipients = Vec::from_array(&env, [RecipientShare { recipient: Address::generate(&env), share_bps: 6000 }, RecipientShare { recipient: Address::generate(&env), share_bps: 4000 }]);
    let fee_recipient = Some(Address::generate(&env));
    let m1 = client.configure_model(&configurer, &tenant, &project, &intent, &FundingModelKind::Grant, &asset, &payer, &recipients, &fee_recipient, &250, &1000, &0);
    let replay = client.configure_model(&configurer, &tenant, &project, &intent, &FundingModelKind::Grant, &asset, &payer, &recipients, &fee_recipient, &250, &1000, &0);
    assert_eq!(m1, replay);
    let bad = Vec::from_array(&env, [RecipientShare { recipient: Address::generate(&env), share_bps: 5000 }]);
    let err = client
        .try_configure_model(&configurer, &tenant, &project, &BytesN::from_array(&env, &[4u8; 32]), &FundingModelKind::Bounty, &asset, &payer, &bad, &fee_recipient, &100, &1000, &0)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::InvalidShare);
}

#[test]
fn unfunded_model_and_authorization_rules() {
    let (env, _admin, configurer, client) = setup();
    let tenant = BytesN::from_array(&env, &[1u8; 32]);
    let project = BytesN::from_array(&env, &[2u8; 32]);
    let intent = BytesN::from_array(&env, &[5u8; 32]);
    let asset = Address::generate(&env);
    let empty = Vec::new(&env);
    let unfunded = client.configure_model(&configurer, &tenant, &project, &intent, &FundingModelKind::Unfunded, &asset, &None, &empty, &None, &0, &0, &0);
    assert_eq!(unfunded.total_amount, 0);
    let stranger = Address::generate(&env);
    let err = client
        .try_configure_model(&stranger, &tenant, &project, &BytesN::from_array(&env, &[6u8; 32]), &FundingModelKind::Unfunded, &asset, &None, &empty, &None, &0, &0, &0)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::Unauthorized);
}
