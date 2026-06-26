#![allow(clippy::unwrap_used, clippy::expect_used)]

extern crate std;

use analytics::{
    aggregation::Aggregator,
    differential_privacy::DifferentialPrivacy,
    homomorphic::{HomomorphicEngine, PaillierPrivateKey, PaillierPublicKey},
    AnalyticsContract, AnalyticsContractClient, MetricDimensions,
};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

fn setup() -> (Env, AnalyticsContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(AnalyticsContract, ());
    let client = AnalyticsContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let aggregator = Address::generate(&env);

    let pub_key = PaillierPublicKey {
        n: 33,
        nn: 1089,
        g: 34,
    };
    let priv_key = PaillierPrivateKey { lambda: 20, mu: 5 };

    client.initialize(&admin, &aggregator, &pub_key, &Some(priv_key));

    (env, client)
}

#[test]
fn test_add_ciphertexts_handles_i128_max_without_overflow() {
    let pub_key = PaillierPublicKey {
        n: 97,
        nn: i128::MAX - 17,
        g: 98,
    };

    let result = HomomorphicEngine::add_ciphertexts(&pub_key, i128::MAX, i128::MAX - 1);

    assert!(result >= 0);
    assert!(result < pub_key.nn);
}

#[test]
fn test_encrypt_handles_i128_max_message_without_panicking() {
    let env = Env::default();
    let pub_key = PaillierPublicKey {
        n: 97,
        nn: i128::MAX - 159,
        g: i128::MAX - 311,
    };

    let ciphertext = HomomorphicEngine::encrypt(&env, &pub_key, i128::MAX);

    assert!(ciphertext >= 0);
    assert!(ciphertext < pub_key.nn);
}

#[test]
fn test_differential_privacy_stays_within_i128_bounds() {
    let env = Env::default();

    let high = DifferentialPrivacy::add_laplace_noise(&env, i128::MAX, 1, 1);
    let low = DifferentialPrivacy::add_laplace_noise(&env, i128::MIN, 1, 1);

    assert!(high <= i128::MAX);
    assert!(low >= i128::MIN);
}

#[test]
fn test_aggregate_average_handles_extreme_inputs() {
    assert_eq!(Aggregator::aggregate_average(i128::MAX, 1), i128::MAX);
    assert_eq!(Aggregator::aggregate_average(i128::MIN, 1), i128::MIN);
    assert_eq!(Aggregator::aggregate_average(i128::MAX, 0), 0);
}

#[test]
fn test_get_trend_supports_u64_max_bucket() {
    let (_env, client) = setup();

    let trend = client.get_trend(
        &symbol_short!("REC_CNT"),
        &Some(symbol_short!("NG")),
        &None,
        &None,
        &u64::MAX,
        &u64::MAX,
    );

    assert_eq!(trend.len(), 1);
    assert_eq!(trend.get(0).unwrap().time_bucket, u64::MAX);
    assert_eq!(trend.get(0).unwrap().value.count, 0);
}

#[test]
fn test_metric_dimensions_accept_u64_max_time_bucket() {
    let (_env, client) = setup();

    let dims = MetricDimensions {
        region: Some(symbol_short!("NG")),
        age_band: None,
        condition: None,
        time_bucket: u64::MAX,
    };

    let metric = client.get_metric(&symbol_short!("REC_CNT"), &dims);

    assert_eq!(metric.count, 0);
    assert_eq!(metric.sum, 0);
}
