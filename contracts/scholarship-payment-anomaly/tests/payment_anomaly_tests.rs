#![cfg(test)]
extern crate std;

use scholarship_payment_anomaly::{
    AlertStatus, AlertSummary, AnomalyCategory, ContractError, PaymentAlert,
    ScholarshipPaymentAnomaly, ScholarshipPaymentAnomalyClient,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, String,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn setup() -> (Env, Address, Address, Address, ScholarshipPaymentAnomalyClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(ScholarshipPaymentAnomaly, ());
    let client = ScholarshipPaymentAnomalyClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let detector = Address::generate(&env);
    let resolver = Address::generate(&env);

    client.init(&admin, &detector, &resolver).unwrap();

    (env, admin, detector, resolver, client)
}

fn s(env: &Env, text: &str) -> String {
    String::from_str(env, text)
}

fn raise_default(
    env: &Env,
    client: &ScholarshipPaymentAnomalyClient,
    detector: &Address,
    payment_ref: &str,
    destination: &Address,
    pause: bool,
) {
    client
        .raise_alert(
            detector,
            &s(env, payment_ref),
            &AnomalyCategory::UnusualDestination,
            &s(env, "Destination wallet newly registered"),
            &s(env, "bafkreigh2evidence"),
            &5_000_i128,
            destination,
            &pause,
            &0, // use default pause window
            &0,
            &0,
        )
        .unwrap();
}

// ── Initialisation ────────────────────────────────────────────────────────────

#[test]
fn init_rejects_double_init() {
    let (env, admin, detector, resolver, client) = setup();
    let result = client.try_init(&admin, &detector, &resolver);
    assert_eq!(result, Err(Ok(ContractError::AlreadyInitialized)));
}

#[test]
fn init_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(ScholarshipPaymentAnomaly, ());
    let client = ScholarshipPaymentAnomalyClient::new(&env, &cid);
    let admin = Address::generate(&env);
    let detector = Address::generate(&env);
    let resolver = Address::generate(&env);
    let before = env.events().all().len();
    client.init(&admin, &detector, &resolver).unwrap();
    assert!(env.events().all().len() > before);
}

// ── Raise alert ───────────────────────────────────────────────────────────────

#[test]
fn raise_alert_creates_record_paused() {
    let (env, _admin, detector, _resolver, client) = setup();
    let dest = Address::generate(&env);
    raise_default(&env, &client, &detector, "pay-001", &dest, true);

    let summary = client.get_alert_summary(&s(&env, "pay-001")).unwrap();
    assert_eq!(summary.status, AlertStatus::PausedPendingReview);
    assert!(summary.payment_paused);
}

#[test]
fn raise_alert_without_pause() {
    let (env, _admin, detector, _resolver, client) = setup();
    let dest = Address::generate(&env);
    raise_default(&env, &client, &detector, "pay-nopaused", &dest, false);

    let summary = client.get_alert_summary(&s(&env, "pay-nopaused")).unwrap();
    assert!(!summary.payment_paused);
}

#[test]
fn raise_alert_duplicate_ref_fails() {
    let (env, _admin, detector, _resolver, client) = setup();
    let dest = Address::generate(&env);
    raise_default(&env, &client, &detector, "pay-dup", &dest, false);
    let result = client.try_raise_alert(
        &detector,
        &s(&env, "pay-dup"),
        &AnomalyCategory::DuplicateLedgerReference,
        &s(&env, "desc"),
        &s(&env, "ev"),
        &1000_i128,
        &dest,
        &false,
        &0,
        &0,
        &0,
    );
    assert_eq!(result, Err(Ok(ContractError::AlertAlreadyExists)));
}

#[test]
fn raise_alert_negative_amount_fails() {
    let (env, _admin, detector, _resolver, client) = setup();
    let dest = Address::generate(&env);
    let result = client.try_raise_alert(
        &detector,
        &s(&env, "pay-neg"),
        &AnomalyCategory::Other,
        &s(&env, "desc"),
        &s(&env, "ev"),
        &-1_i128,
        &dest,
        &false,
        &0,
        &0,
        &0,
    );
    assert_eq!(result, Err(Ok(ContractError::InvalidAmount)));
}

#[test]
fn raise_alert_payment_ref_too_long_fails() {
    let (env, _admin, detector, _resolver, client) = setup();
    let dest = Address::generate(&env);
    let long_ref = String::from_str(&env, &"r".repeat(65));
    let result = client.try_raise_alert(
        &detector,
        &long_ref,
        &AnomalyCategory::Other,
        &s(&env, "desc"),
        &s(&env, "ev"),
        &100_i128,
        &dest,
        &false,
        &0,
        &0,
        &0,
    );
    assert_eq!(result, Err(Ok(ContractError::PaymentRefTooLong)));
}

#[test]
fn raise_alert_unauthorized_non_detector_fails() {
    let (env, _admin, _detector, _resolver, client) = setup();
    let dest = Address::generate(&env);
    let stranger = Address::generate(&env);
    let result = client.try_raise_alert(
        &stranger,
        &s(&env, "pay-unauth"),
        &AnomalyCategory::Other,
        &s(&env, "desc"),
        &s(&env, "ev"),
        &100_i128,
        &dest,
        &false,
        &0,
        &0,
        &0,
    );
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

// ── is_payment_paused ─────────────────────────────────────────────────────────

#[test]
fn is_payment_paused_true_for_active_pause() {
    let (env, _admin, detector, _resolver, client) = setup();
    let dest = Address::generate(&env);
    raise_default(&env, &client, &detector, "pay-paused-check", &dest, true);
    assert!(client.is_payment_paused(&s(&env, "pay-paused-check")));
}

#[test]
fn is_payment_paused_false_for_unknown_ref() {
    let (env, _admin, _detector, _resolver, client) = setup();
    assert!(!client.is_payment_paused(&s(&env, "ghost-pay")));
}

#[test]
fn is_payment_paused_false_after_clear() {
    let (env, _admin, detector, resolver, client) = setup();
    let dest = Address::generate(&env);
    raise_default(&env, &client, &detector, "pay-clear-test", &dest, true);
    assert!(client.is_payment_paused(&s(&env, "pay-clear-test")));

    client
        .resolve_alert(
            &resolver,
            &s(&env, "pay-clear-test"),
            &AlertStatus::Cleared,
            &s(&env, "No anomaly found after investigation"),
        )
        .unwrap();
    assert!(!client.is_payment_paused(&s(&env, "pay-clear-test")));
}

#[test]
fn is_payment_paused_false_when_pause_expired() {
    let (env, _admin, detector, _resolver, client) = setup();
    let dest = Address::generate(&env);
    // Raise with a 100-second pause window
    client
        .raise_alert(
            &detector,
            &s(&env, "pay-expire"),
            &AnomalyCategory::RapidWalletChange,
            &s(&env, "desc"),
            &s(&env, "ev"),
            &500_i128,
            &dest,
            &true,
            &100, // 100-second window
            &3,
            &0,
        )
        .unwrap();

    assert!(client.is_payment_paused(&s(&env, "pay-expire")));

    // Advance time past the window
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 101);
    assert!(
        !client.is_payment_paused(&s(&env, "pay-expire")),
        "pause should have expired"
    );
}

// ── Resolve alert ─────────────────────────────────────────────────────────────

#[test]
fn resolve_alert_cleared_releases_paused_amount() {
    let (env, _admin, detector, resolver, client) = setup();
    let dest = Address::generate(&env);
    raise_default(&env, &client, &detector, "pay-rel", &dest, true);

    let before = client.total_paused_amount();
    assert!(before >= 5_000);

    client
        .resolve_alert(
            &resolver,
            &s(&env, "pay-rel"),
            &AlertStatus::Cleared,
            &s(&env, "Clean payment, investigation complete"),
        )
        .unwrap();

    assert_eq!(
        client.total_paused_amount(),
        before - 5_000,
        "paused amount decremented after clearing"
    );
}

#[test]
fn resolve_alert_confirmed_keeps_amount_paused() {
    let (env, _admin, detector, resolver, client) = setup();
    let dest = Address::generate(&env);
    raise_default(&env, &client, &detector, "pay-conf", &dest, true);

    let before = client.total_paused_amount();
    client
        .resolve_alert(
            &resolver,
            &s(&env, "pay-conf"),
            &AlertStatus::Confirmed,
            &s(&env, "Suspicious pattern confirmed; escalate to compliance"),
        )
        .unwrap();

    // Confirmed does not release; amount stays registered
    assert_eq!(client.total_paused_amount(), before);
}

#[test]
fn resolve_already_resolved_alert_fails() {
    let (env, _admin, detector, resolver, client) = setup();
    let dest = Address::generate(&env);
    raise_default(&env, &client, &detector, "pay-double-res", &dest, false);
    client
        .resolve_alert(
            &resolver,
            &s(&env, "pay-double-res"),
            &AlertStatus::Cleared,
            &s(&env, "ok"),
        )
        .unwrap();
    let result = client.try_resolve_alert(
        &resolver,
        &s(&env, "pay-double-res"),
        &AlertStatus::Confirmed,
        &s(&env, "re-resolve"),
    );
    assert_eq!(result, Err(Ok(ContractError::AlertAlreadyResolved)));
}

#[test]
fn resolve_with_invalid_status_fails() {
    let (env, _admin, detector, resolver, client) = setup();
    let dest = Address::generate(&env);
    raise_default(&env, &client, &detector, "pay-inv-st", &dest, false);
    let result = client.try_resolve_alert(
        &resolver,
        &s(&env, "pay-inv-st"),
        &AlertStatus::PausedPendingReview,
        &s(&env, "invalid"),
    );
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn resolve_unauthorized_non_resolver_fails() {
    let (env, _admin, detector, _resolver, client) = setup();
    let dest = Address::generate(&env);
    raise_default(&env, &client, &detector, "pay-rv-unauth", &dest, false);
    let stranger = Address::generate(&env);
    let result = client.try_resolve_alert(
        &stranger,
        &s(&env, "pay-rv-unauth"),
        &AlertStatus::Cleared,
        &s(&env, "notes"),
    );
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn resolve_nonexistent_alert_fails() {
    let (env, _admin, _detector, resolver, client) = setup();
    let result = client.try_resolve_alert(
        &resolver,
        &s(&env, "ghost"),
        &AlertStatus::Cleared,
        &s(&env, "no record"),
    );
    assert_eq!(result, Err(Ok(ContractError::AlertNotFound)));
}

// ── Global counters ────────────────────────────────────────────────────────────

#[test]
fn total_alerts_increments_per_raise() {
    let (env, _admin, detector, _resolver, client) = setup();
    let dest = Address::generate(&env);
    assert_eq!(client.total_alerts(), 0);
    raise_default(&env, &client, &detector, "cnt-1", &dest, false);
    raise_default(&env, &client, &detector, "cnt-2", &dest, false);
    assert_eq!(client.total_alerts(), 2);
}

#[test]
fn total_paused_amount_accumulates() {
    let (env, _admin, detector, _resolver, client) = setup();
    let dest = Address::generate(&env);
    raise_default(&env, &client, &detector, "acc-1", &dest, true); // +5_000
    raise_default(&env, &client, &detector, "acc-2", &dest, true); // +5_000
    assert_eq!(client.total_paused_amount(), 10_000);
}

// ── All anomaly categories ────────────────────────────────────────────────────

#[test]
fn all_anomaly_categories_can_be_raised() {
    let (env, _admin, detector, _resolver, client) = setup();
    let dest = Address::generate(&env);
    let categories = [
        (AnomalyCategory::UnusualDestination, "cat-ud"),
        (AnomalyCategory::RapidWalletChange, "cat-rwc"),
        (AnomalyCategory::DuplicateLedgerReference, "cat-dlr"),
        (AnomalyCategory::SuspiciousSplit, "cat-ss"),
        (AnomalyCategory::RepeatedFailure, "cat-rf"),
        (AnomalyCategory::Other, "cat-oth"),
    ];
    for (cat, id) in categories.iter() {
        client
            .raise_alert(
                &detector,
                &s(&env, id),
                cat,
                &s(&env, "description"),
                &s(&env, "evidence"),
                &100_i128,
                &dest,
                &false,
                &0,
                &0,
                &0,
            )
            .unwrap();
    }
}

// ── Wallet-change velocity ────────────────────────────────────────────────────

#[test]
fn wallet_change_velocity_stored_in_alert() {
    let (env, _admin, detector, _resolver, client) = setup();
    let dest = Address::generate(&env);
    client
        .raise_alert(
            &detector,
            &s(&env, "vel-test"),
            &AnomalyCategory::RapidWalletChange,
            &s(&env, "Wallet changed 5 times in 1 hour"),
            &s(&env, "evidence"),
            &2_500_i128,
            &dest,
            &true,
            &0,
            &5, // velocity = 5
            &2, // 2 prior failures
        )
        .unwrap();
    let full = client.get_alert_full(&s(&env, "vel-test")).unwrap();
    assert_eq!(full.wallet_change_velocity, 5);
    assert_eq!(full.retry_count, 2);
}

// ── Pause / unpause contract ──────────────────────────────────────────────────

#[test]
fn contract_pause_blocks_raise_alert() {
    let (env, admin, detector, _resolver, client) = setup();
    let dest = Address::generate(&env);
    client.pause(&admin).unwrap();
    let result = client.try_raise_alert(
        &detector,
        &s(&env, "blocked"),
        &AnomalyCategory::Other,
        &s(&env, "desc"),
        &s(&env, "ev"),
        &100_i128,
        &dest,
        &false,
        &0,
        &0,
        &0,
    );
    assert_eq!(result, Err(Ok(ContractError::ContractPaused)));
}

#[test]
fn only_admin_can_pause_contract() {
    let (env, _admin, _detector, _resolver, client) = setup();
    let stranger = Address::generate(&env);
    let result = client.try_pause(&stranger);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

// ── Audited resolution ────────────────────────────────────────────────────────

#[test]
fn resolution_notes_stored_and_resolver_recorded() {
    let (env, _admin, detector, resolver, client) = setup();
    let dest = Address::generate(&env);
    raise_default(&env, &client, &detector, "audit-pay", &dest, false);
    client
        .resolve_alert(
            &resolver,
            &s(&env, "audit-pay"),
            &AlertStatus::Confirmed,
            &s(&env, "Escalated per SOP-ANOMALY-001; report hash bafkrei..."),
        )
        .unwrap();
    let full = client.get_alert_full(&s(&env, "audit-pay")).unwrap();
    assert_eq!(full.resolved_by, Some(resolver));
    assert!(!full.resolution_notes.is_empty());
    assert!(full.resolved_at > 0);
}
