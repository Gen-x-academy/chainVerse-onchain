#![cfg(test)]
extern crate std;

use scholarship_fraud_signals::{
    ContractError, FraudSignalSummary, SignalCategory, SignalStatus,
    ScholarshipFraudSignals, ScholarshipFraudSignalsClient,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, String,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn setup() -> (Env, Address, Address, Address, ScholarshipFraudSignalsClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(ScholarshipFraudSignals, ());
    let client = ScholarshipFraudSignalsClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let detector = Address::generate(&env);
    let reviewer = Address::generate(&env);

    client.init(&admin, &detector, &reviewer).unwrap();

    (env, admin, detector, reviewer, client)
}

fn s(env: &Env, text: &str) -> String {
    String::from_str(env, text)
}

fn raise_default(
    env: &Env,
    client: &ScholarshipFraudSignalsClient,
    detector: &Address,
    app_id: &str,
) {
    client
        .raise_signal(
            detector,
            &s(env, app_id),
            &SignalCategory::LikelyDuplicate,
            &s(env, "Detected identical submission content"),
            &s(env, "bafkreigh2akiscaildc4i53lx4d7xjdpd"),
            &75,
        )
        .unwrap();
}

// ── Initialisation ────────────────────────────────────────────────────────────

#[test]
fn init_rejects_double_init() {
    let (env, admin, detector, reviewer, client) = setup();
    let result = client.try_init(&admin, &detector, &reviewer);
    assert_eq!(result, Err(Ok(ContractError::AlreadyInitialized)));
}

#[test]
fn init_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ScholarshipFraudSignals, ());
    let client = ScholarshipFraudSignalsClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let detector = Address::generate(&env);
    let reviewer = Address::generate(&env);

    let before = env.events().all().len();
    client.init(&admin, &detector, &reviewer).unwrap();
    assert!(env.events().all().len() > before);
}

// ── Raise signal ──────────────────────────────────────────────────────────────

#[test]
fn raise_signal_creates_record_with_flagged_status() {
    let (env, _admin, detector, _reviewer, client) = setup();
    raise_default(&env, &client, &detector, "app-001");
    let summary = client.get_signal_summary(&s(&env, "app-001")).unwrap();
    assert_eq!(summary.status, SignalStatus::Flagged);
    assert_eq!(summary.risk_score, 75);
}

#[test]
fn raise_signal_duplicate_app_id_fails() {
    let (env, _admin, detector, _reviewer, client) = setup();
    raise_default(&env, &client, &detector, "app-dup");
    let result = client.try_raise_signal(
        &detector,
        &s(&env, "app-dup"),
        &SignalCategory::DocumentReuse,
        &s(&env, "description"),
        &s(&env, "evidence"),
        &50,
    );
    assert_eq!(result, Err(Ok(ContractError::SignalAlreadyExists)));
}

#[test]
fn raise_signal_invalid_score_over_100_fails() {
    let (env, _admin, detector, _reviewer, client) = setup();
    let result = client.try_raise_signal(
        &detector,
        &s(&env, "app-bad-score"),
        &SignalCategory::Other,
        &s(&env, "desc"),
        &s(&env, "ev"),
        &101,
    );
    assert_eq!(result, Err(Ok(ContractError::InvalidScore)));
}

#[test]
fn raise_signal_score_zero_is_allowed() {
    let (env, _admin, detector, _reviewer, client) = setup();
    client
        .raise_signal(
            &detector,
            &s(&env, "app-zero-score"),
            &SignalCategory::Other,
            &s(&env, "monitoring only"),
            &s(&env, ""),
            &0,
        )
        .unwrap();
    let summary = client.get_signal_summary(&s(&env, "app-zero-score")).unwrap();
    assert_eq!(summary.risk_score, 0);
}

#[test]
fn raise_signal_unauthorized_for_non_detector() {
    let (env, _admin, _detector, _reviewer, client) = setup();
    let stranger = Address::generate(&env);
    let result = client.try_raise_signal(
        &stranger,
        &s(&env, "app-unauth"),
        &SignalCategory::LikelyDuplicate,
        &s(&env, "desc"),
        &s(&env, "ev"),
        &50,
    );
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn raise_signal_app_id_too_long_fails() {
    let (env, _admin, detector, _reviewer, client) = setup();
    let long_id = String::from_str(&env, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"); // 65 chars
    let result = client.try_raise_signal(
        &detector,
        &long_id,
        &SignalCategory::Other,
        &s(&env, "desc"),
        &s(&env, "ev"),
        &10,
    );
    assert_eq!(result, Err(Ok(ContractError::AppIdTooLong)));
}

#[test]
fn raise_signal_description_too_long_fails() {
    let (env, _admin, detector, _reviewer, client) = setup();
    // 513 bytes
    let long_desc = String::from_str(&env, &"x".repeat(513));
    let result = client.try_raise_signal(
        &detector,
        &s(&env, "app-long-desc"),
        &SignalCategory::Other,
        &long_desc,
        &s(&env, "ev"),
        &10,
    );
    assert_eq!(result, Err(Ok(ContractError::DescriptionTooLong)));
}

// ── Update signal ─────────────────────────────────────────────────────────────

#[test]
fn update_signal_revises_evidence_and_score() {
    let (env, _admin, detector, _reviewer, client) = setup();
    raise_default(&env, &client, &detector, "app-upd");
    client
        .update_signal(
            &detector,
            &s(&env, "app-upd"),
            &s(&env, "bafkreinewevidence"),
            &90,
        )
        .unwrap();
    let full = client.get_signal_full(&s(&env, "app-upd")).unwrap();
    assert_eq!(full.risk_score, 90);
}

#[test]
fn update_signal_nonexistent_fails() {
    let (env, _admin, detector, _reviewer, client) = setup();
    let result = client.try_update_signal(
        &detector,
        &s(&env, "ghost"),
        &s(&env, "ev"),
        &50,
    );
    assert_eq!(result, Err(Ok(ContractError::SignalNotFound)));
}

// ── Resolve signal ────────────────────────────────────────────────────────────

#[test]
fn resolve_signal_confirmed() {
    let (env, _admin, detector, reviewer, client) = setup();
    raise_default(&env, &client, &detector, "app-conf");
    client
        .resolve_signal(
            &reviewer,
            &s(&env, "app-conf"),
            &SignalStatus::Confirmed,
            &s(&env, "Verified by cross-referencing submission timestamps"),
        )
        .unwrap();
    let summary = client.get_signal_summary(&s(&env, "app-conf")).unwrap();
    assert_eq!(summary.status, SignalStatus::Confirmed);
}

#[test]
fn resolve_signal_dismissed() {
    let (env, _admin, detector, reviewer, client) = setup();
    raise_default(&env, &client, &detector, "app-dis");
    client
        .resolve_signal(
            &reviewer,
            &s(&env, "app-dis"),
            &SignalStatus::Dismissed,
            &s(&env, "False positive: same template used by institution"),
        )
        .unwrap();
    let summary = client.get_signal_summary(&s(&env, "app-dis")).unwrap();
    assert_eq!(summary.status, SignalStatus::Dismissed);
}

#[test]
fn resolve_signal_unauthorized_for_non_reviewer() {
    let (env, _admin, detector, _reviewer, client) = setup();
    raise_default(&env, &client, &detector, "app-rv-unauth");
    let stranger = Address::generate(&env);
    let result = client.try_resolve_signal(
        &stranger,
        &s(&env, "app-rv-unauth"),
        &SignalStatus::Confirmed,
        &s(&env, "notes"),
    );
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn resolve_signal_invalid_terminal_status_fails() {
    let (env, _admin, detector, reviewer, client) = setup();
    raise_default(&env, &client, &detector, "app-inv-st");
    // Cannot resolve to Flagged (not a terminal resolution)
    let result = client.try_resolve_signal(
        &reviewer,
        &s(&env, "app-inv-st"),
        &SignalStatus::Flagged,
        &s(&env, "notes"),
    );
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn resolve_signal_nonexistent_fails() {
    let (env, _admin, _detector, reviewer, client) = setup();
    let result = client.try_resolve_signal(
        &reviewer,
        &s(&env, "ghost"),
        &SignalStatus::Dismissed,
        &s(&env, "no record"),
    );
    assert_eq!(result, Err(Ok(ContractError::SignalNotFound)));
}

// ── No-auto-reject guarantee ──────────────────────────────────────────────────

/// Core invariant: raising a signal does NOT produce a Confirmed or terminal
/// status; the signal sits in Flagged and requires human action.
#[test]
fn signal_status_is_flagged_not_confirmed_after_raise() {
    let (env, _admin, detector, _reviewer, client) = setup();
    raise_default(&env, &client, &detector, "no-auto-reject");
    let summary = client.get_signal_summary(&s(&env, "no-auto-reject")).unwrap();
    assert_eq!(
        summary.status,
        SignalStatus::Flagged,
        "Signal must not auto-confirm — human review required"
    );
}

// ── Appeal lifecycle ──────────────────────────────────────────────────────────

#[test]
fn file_appeal_transitions_to_under_appeal() {
    let (env, admin, detector, _reviewer, client) = setup();
    raise_default(&env, &client, &detector, "app-appeal");
    client
        .file_appeal(
            &admin,
            &s(&env, "app-appeal"),
            &s(&env, "bafkreiapplicantrebuttalcid"),
        )
        .unwrap();
    let summary = client.get_signal_summary(&s(&env, "app-appeal")).unwrap();
    assert_eq!(summary.status, SignalStatus::UnderAppeal);
    assert!(summary.has_appeal);
}

#[test]
fn file_duplicate_appeal_fails() {
    let (env, admin, detector, _reviewer, client) = setup();
    raise_default(&env, &client, &detector, "app-dup-appeal");
    client
        .file_appeal(&admin, &s(&env, "app-dup-appeal"), &s(&env, "cid-1"))
        .unwrap();
    let result = client.try_file_appeal(&admin, &s(&env, "app-dup-appeal"), &s(&env, "cid-2"));
    assert_eq!(result, Err(Ok(ContractError::AppealAlreadyFiled)));
}

#[test]
fn appeal_upheld_resolves_under_appeal_signal() {
    let (env, admin, detector, reviewer, client) = setup();
    raise_default(&env, &client, &detector, "app-upheld");
    client
        .file_appeal(&admin, &s(&env, "app-upheld"), &s(&env, "rebut-cid"))
        .unwrap();
    client
        .resolve_signal(
            &reviewer,
            &s(&env, "app-upheld"),
            &SignalStatus::AppealUpheld,
            &s(&env, "Signal was incorrect; applicant is clear"),
        )
        .unwrap();
    let summary = client.get_signal_summary(&s(&env, "app-upheld")).unwrap();
    assert_eq!(summary.status, SignalStatus::AppealUpheld);
}

#[test]
fn appeal_rejected_keeps_signal_standing() {
    let (env, admin, detector, reviewer, client) = setup();
    raise_default(&env, &client, &detector, "app-rejected-appeal");
    client
        .file_appeal(&admin, &s(&env, "app-rejected-appeal"), &s(&env, "rebut-cid"))
        .unwrap();
    client
        .resolve_signal(
            &reviewer,
            &s(&env, "app-rejected-appeal"),
            &SignalStatus::AppealRejected,
            &s(&env, "Appeal rebuttal insufficient"),
        )
        .unwrap();
    let summary = client
        .get_signal_summary(&s(&env, "app-rejected-appeal"))
        .unwrap();
    assert_eq!(summary.status, SignalStatus::AppealRejected);
}

// ── SignalNotFlagged guard ────────────────────────────────────────────────────

#[test]
fn cannot_resolve_already_confirmed_signal() {
    let (env, _admin, detector, reviewer, client) = setup();
    raise_default(&env, &client, &detector, "app-double-resolve");
    client
        .resolve_signal(
            &reviewer,
            &s(&env, "app-double-resolve"),
            &SignalStatus::Confirmed,
            &s(&env, "confirmed"),
        )
        .unwrap();
    // Attempting a second resolution should fail
    let result = client.try_resolve_signal(
        &reviewer,
        &s(&env, "app-double-resolve"),
        &SignalStatus::Dismissed,
        &s(&env, "changed mind"),
    );
    assert_eq!(result, Err(Ok(ContractError::SignalNotFlagged)));
}

// ── Pause ─────────────────────────────────────────────────────────────────────

#[test]
fn pause_blocks_raise_signal() {
    let (env, admin, detector, _reviewer, client) = setup();
    client.pause(&admin).unwrap();
    let result = client.try_raise_signal(
        &detector,
        &s(&env, "blocked"),
        &SignalCategory::Other,
        &s(&env, "desc"),
        &s(&env, "ev"),
        &10,
    );
    assert_eq!(result, Err(Ok(ContractError::ContractPaused)));
}

#[test]
fn only_admin_can_pause() {
    let (env, _admin, _detector, _reviewer, client) = setup();
    let stranger = Address::generate(&env);
    let result = client.try_pause(&stranger);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

// ── Explainability — summary does not expose raw evidence ────────────────────

#[test]
fn signal_summary_does_not_contain_evidence_ref() {
    let (env, _admin, detector, _reviewer, client) = setup();
    raise_default(&env, &client, &detector, "app-explainability");
    let summary = client.get_signal_summary(&s(&env, "app-explainability")).unwrap();
    // FraudSignalSummary has no evidence_ref field — verified at compile time.
    // Check description is present (explainability requirement)
    assert!(!summary.description.is_empty());
}

// ── Adversarial: all signal categories covered ───────────────────────────────

#[test]
fn all_signal_categories_can_be_raised() {
    let (env, _admin, detector, _reviewer, client) = setup();
    let categories = [
        (SignalCategory::LikelyDuplicate, "cat-dup"),
        (SignalCategory::DocumentReuse, "cat-doc"),
        (SignalCategory::ManipulatedEvidence, "cat-manip"),
        (SignalCategory::CoordinatedSubmission, "cat-coord"),
        (SignalCategory::Other, "cat-other"),
    ];
    for (cat, id) in categories.iter() {
        client
            .raise_signal(
                &detector,
                &s(&env, id),
                cat,
                &s(&env, "description"),
                &s(&env, "evidence"),
                &50,
            )
            .unwrap();
    }
}
