#![cfg(test)]

use crate::{AlertStatus, AnomalyCategory, ContractError, ScholarshipPaymentAnomaly};
use soroban_sdk::{testutils::Address as _, Address, Env, String};

fn setup(env: &Env) -> (Address, Address, Address, soroban_sdk::Address) {
    let cid = env.register(ScholarshipPaymentAnomaly, ());
    let admin = Address::generate(env);
    let detector = Address::generate(env);
    let resolver = Address::generate(env);
    let client = crate::ScholarshipPaymentAnomalyClient::new(env, &cid);
    client.init(&admin, &detector, &resolver).unwrap();
    (admin, detector, resolver, cid)
}

fn s(env: &Env, t: &str) -> String {
    String::from_str(env, t)
}

/// Smoke: raise → resolve → is_payment_paused = false.
#[test]
fn smoke_raise_resolve() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, detector, resolver, cid) = setup(&env);
    let client = crate::ScholarshipPaymentAnomalyClient::new(&env, &cid);
    let dest = Address::generate(&env);

    client
        .raise_alert(
            &detector,
            &s(&env, "pay-s"),
            &AnomalyCategory::UnusualDestination,
            &s(&env, "desc"),
            &s(&env, "ev"),
            &1_000_i128,
            &dest,
            &true,
            &0,
            &0,
            &0,
        )
        .unwrap();
    assert!(client.is_payment_paused(&s(&env, "pay-s")));

    client
        .resolve_alert(
            &resolver,
            &s(&env, "pay-s"),
            &AlertStatus::Cleared,
            &s(&env, "investigation complete"),
        )
        .unwrap();
    assert!(!client.is_payment_paused(&s(&env, "pay-s")));
}

/// Unit: uninitialised contract returns NotInitialized.
#[test]
fn requires_init_before_raise() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(ScholarshipPaymentAnomaly, ());
    let client = crate::ScholarshipPaymentAnomalyClient::new(&env, &cid);
    let dest = Address::generate(&env);
    let result = client.try_raise_alert(
        &Address::generate(&env),
        &s(&env, "x"),
        &AnomalyCategory::Other,
        &s(&env, "d"),
        &s(&env, "e"),
        &10_i128,
        &dest,
        &false,
        &0,
        &0,
        &0,
    );
    assert_eq!(result, Err(Ok(ContractError::NotInitialized)));
}
