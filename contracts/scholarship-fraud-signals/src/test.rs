#![cfg(test)]

use crate::{ContractError, ScholarshipFraudSignals, SignalCategory, SignalStatus};
use soroban_sdk::{testutils::Address as _, Address, Env, String};

fn setup(env: &Env) -> (Address, Address, Address, soroban_sdk::Address) {
    let cid = env.register(ScholarshipFraudSignals, ());
    let admin = Address::generate(env);
    let detector = Address::generate(env);
    let reviewer = Address::generate(env);
    let client = crate::ScholarshipFraudSignalsClient::new(env, &cid);
    client.init(&admin, &detector, &reviewer).unwrap();
    (admin, detector, reviewer, cid)
}

fn s(env: &Env, t: &str) -> String {
    String::from_str(env, t)
}

/// Smoke: raise → resolve lifecycle.
#[test]
fn smoke_raise_resolve() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, detector, reviewer, cid) = setup(&env);
    let client = crate::ScholarshipFraudSignalsClient::new(&env, &cid);

    client
        .raise_signal(
            &detector,
            &s(&env, "app-s"),
            &SignalCategory::LikelyDuplicate,
            &s(&env, "desc"),
            &s(&env, "ev"),
            &60,
        )
        .unwrap();
    client
        .resolve_signal(
            &reviewer,
            &s(&env, "app-s"),
            &SignalStatus::Dismissed,
            &s(&env, "false positive"),
        )
        .unwrap();
    let sum = client.get_signal_summary(&s(&env, "app-s")).unwrap();
    assert_eq!(sum.status, SignalStatus::Dismissed);
}

/// Unit: uninitialised contract raises NotInitialized.
#[test]
fn requires_init_before_raise() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(ScholarshipFraudSignals, ());
    let client = crate::ScholarshipFraudSignalsClient::new(&env, &cid);
    let result = client.try_raise_signal(
        &Address::generate(&env),
        &s(&env, "x"),
        &SignalCategory::Other,
        &s(&env, "d"),
        &s(&env, "e"),
        &10,
    );
    assert_eq!(result, Err(Ok(ContractError::NotInitialized)));
}
