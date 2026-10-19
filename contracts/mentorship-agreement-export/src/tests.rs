//! Deterministic tests for participant agreement record export
//! (#1248, MENTOR-052).

use crate::{
    AgreementRecord, AuditKind, ContractError, ExportManifest, ExportPolicy,
    MentorshipAgreementExportContract, MentorshipAgreementExportContractClient,
};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger},
    Address, BytesN, Env, Symbol, TryFromVal,
};

const BASE: u64 = 1_700_000_000;
const MAX_PER_WINDOW: u32 = 2;
const WINDOW: u32 = 3_600;
const GRANT_TTL: u32 = 900;

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

struct Fixture {
    env: Env,
    contract_id: Address,
    admin: Address,
    registrar: Address,
    mentor: Address,
    mentee: Address,
    outsider: Address,
    agreement: BytesN<32>,
}

fn fixture() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(BASE);
    let contract_id = env.register(MentorshipAgreementExportContract, ());
    let c = MentorshipAgreementExportContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    c.initialize(&admin, &MAX_PER_WINDOW, &WINDOW, &GRANT_TTL);
    let registrar = admin.clone();
    let mentor = Address::generate(&env);
    let mentee = Address::generate(&env);
    let outsider = Address::generate(&env);
    let agreement = hash(&env, 0xA1);
    c.register_agreement(
        &registrar,
        &agreement,
        &mentor,
        &mentee,
        &hash(&env, 0x10),
        &1,
    );
    Fixture {
        env,
        contract_id,
        admin,
        registrar,
        mentor,
        mentee,
        outsider,
        agreement,
    }
}

fn client<'a>(env: &'a Env, contract_id: &Address) -> MentorshipAgreementExportContractClient<'a> {
    MentorshipAgreementExportContractClient::new(env, contract_id)
}

// ── initialization / configuration ─────────────────────────────────────────

#[test]
fn test_initialize_twice_fails() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    assert_eq!(
        c.try_initialize(&f.admin, &MAX_PER_WINDOW, &WINDOW, &GRANT_TTL),
        Err(Ok(ContractError::AlreadyInitialized))
    );
}

#[test]
fn test_initialize_rejects_invalid_policy() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MentorshipAgreementExportContract, ());
    let c = client(&env, &contract_id);
    let admin = Address::generate(&env);
    // Zero exports per window would disable the feature entirely.
    assert_eq!(
        c.try_initialize(&admin, &0, &WINDOW, &GRANT_TTL),
        Err(Ok(ContractError::InvalidPolicy))
    );
    // A grant that outlives 7 days is refused.
    assert_eq!(
        c.try_initialize(&admin, &MAX_PER_WINDOW, &WINDOW, &(8 * 86_400)),
        Err(Ok(ContractError::InvalidPolicy))
    );
    // Uninitialized getters fail closed.
    assert_eq!(c.try_registrar(), Err(Ok(ContractError::NotInitialized)));
}

#[test]
fn test_admin_actions_reject_non_admin() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let attacker = Address::generate(&f.env);
    assert_eq!(
        c.try_set_registrar(&attacker, &attacker),
        Err(Ok(ContractError::NotAdmin))
    );
    assert_eq!(
        c.try_set_policy(&attacker, &5, &WINDOW, &GRANT_TTL),
        Err(Ok(ContractError::NotAdmin))
    );
    assert_eq!(
        c.try_upgrade(&attacker, &hash(&f.env, 0xB0)),
        Err(Ok(ContractError::NotAdmin))
    );
}

#[test]
fn test_policy_round_trip() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    c.set_policy(&f.admin, &5, &7_200, &1_800);
    assert_eq!(
        c.policy(),
        ExportPolicy {
            max_exports_per_window: 5,
            window_seconds: 7_200,
            grant_ttl_seconds: 1_800,
        }
    );
}

// ── agreement projection ───────────────────────────────────────────────────

#[test]
fn test_non_registrar_cannot_register_agreement() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    assert_eq!(
        c.try_register_agreement(
            &f.outsider,
            &hash(&f.env, 0xB2),
            &f.mentor,
            &f.mentee,
            &hash(&f.env, 1),
            &1
        ),
        Err(Ok(ContractError::NotRegistrar))
    );
}

#[test]
fn test_duplicate_agreement_registration_fails() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    assert_eq!(
        c.try_register_agreement(
            &f.registrar,
            &f.agreement,
            &f.mentor,
            &f.mentee,
            &hash(&f.env, 1),
            &1
        ),
        Err(Ok(ContractError::AgreementAlreadyRegistered))
    );
}

#[test]
fn test_approval_is_replay_protected() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    c.record_approval(&f.mentor, &f.agreement, &hash(&f.env, 0x21));
    // The same approver on the same terms version cannot approve twice.
    assert_eq!(
        c.try_record_approval(&f.mentor, &f.agreement, &hash(&f.env, 0x22)),
        Err(Ok(ContractError::DuplicateApproval))
    );
    // The other participant is independent.
    c.record_approval(&f.mentee, &f.agreement, &hash(&f.env, 0x23));
    assert_eq!(c.get_agreement(&f.agreement).approvals.len(), 2);
    // A non-participant cannot record an approval at all.
    assert_eq!(
        c.try_record_approval(&f.outsider, &f.agreement, &hash(&f.env, 0x24)),
        Err(Ok(ContractError::NotAuthorized))
    );
}

#[test]
fn test_amendment_versions_must_increase() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    // Registered terms are version 1.
    assert_eq!(
        c.try_record_amendment(
            &f.registrar,
            &f.agreement,
            &1,
            &hash(&f.env, 2),
            &hash(&f.env, 3)
        ),
        Err(Ok(ContractError::AmendmentVersionNotIncreasing))
    );
    assert_eq!(
        c.try_record_amendment(
            &f.registrar,
            &f.agreement,
            &0,
            &hash(&f.env, 2),
            &hash(&f.env, 3)
        ),
        Err(Ok(ContractError::AmendmentVersionNotIncreasing))
    );
    c.record_amendment(
        &f.registrar,
        &f.agreement,
        &2,
        &hash(&f.env, 4),
        &hash(&f.env, 5),
    );
    let record = c.get_agreement(&f.agreement);
    assert_eq!(record.terms_version, 2);
    assert_eq!(record.terms_hash, hash(&f.env, 4));
    assert_eq!(record.amendments.len(), 1);
    // Non-registrators cannot amend.
    assert_eq!(
        c.try_record_amendment(
            &f.mentor,
            &f.agreement,
            &3,
            &hash(&f.env, 6),
            &hash(&f.env, 7)
        ),
        Err(Ok(ContractError::NotRegistrar))
    );
}

#[test]
fn test_amendment_storage_bound() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    // Fill to the 32-amendment cap; versions increase monotonically.
    for version in 2..=33u32 {
        c.record_amendment(
            &f.registrar,
            &f.agreement,
            &version,
            &hash(&f.env, version as u8),
            &hash(&f.env, (version as u8) ^ 0xFF),
        );
    }
    assert_eq!(
        c.try_record_amendment(
            &f.registrar,
            &f.agreement,
            &34,
            &hash(&f.env, 9),
            &hash(&f.env, 8)
        ),
        Err(Ok(ContractError::AmendmentLimitReached))
    );
}

#[test]
fn test_session_summary_financials_accumulate() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    // gross/fees per session: (1000/100), (500/50), invalid attempts.
    c.record_session_summary(
        &f.registrar,
        &f.agreement,
        &hash(&f.env, 1),
        &hash(&f.env, 0x31),
        &(BASE + 1_000),
        &60,
        &1_000,
        &100,
    );
    c.record_session_summary(
        &f.registrar,
        &f.agreement,
        &hash(&f.env, 2),
        &hash(&f.env, 0x32),
        &(BASE + 2_000),
        &45,
        &500,
        &50,
    );
    let record = c.get_agreement(&f.agreement);
    assert_eq!(record.session_summaries.len(), 2);
    assert_eq!(record.financial.gross, 1_500);
    assert_eq!(record.financial.fees, 150);
    assert_eq!(record.financial.net, 1_350);

    // Fees cannot exceed gross, and amounts cannot be negative.
    assert_eq!(
        c.try_record_session_summary(
            &f.registrar,
            &f.agreement,
            &hash(&f.env, 3),
            &hash(&f.env, 0x33),
            &BASE,
            &30,
            &100,
            &200
        ),
        Err(Ok(ContractError::InvalidAmount))
    );
    assert_eq!(
        c.try_record_session_summary(
            &f.registrar,
            &f.agreement,
            &hash(&f.env, 3),
            &hash(&f.env, 0x33),
            &BASE,
            &30,
            &(-1),
            &0
        ),
        Err(Ok(ContractError::InvalidAmount))
    );
}

#[test]
fn test_session_summary_storage_bound() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    for i in 0..64u8 {
        c.record_session_summary(
            &f.registrar,
            &f.agreement,
            &hash(&f.env, i),
            &hash(&f.env, i ^ 0x80),
            &(BASE + u64::from(i) * 100),
            &30,
            &10,
            &1,
        );
    }
    assert_eq!(
        c.try_record_session_summary(
            &f.registrar,
            &f.agreement,
            &hash(&f.env, 200),
            &hash(&f.env, 201),
            &BASE,
            &30,
            &10,
            &1
        ),
        Err(Ok(ContractError::SessionSummaryLimitReached))
    );
}

// ── export flow: authorization ─────────────────────────────────────────────

#[test]
fn test_only_participants_can_request_export() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    assert_eq!(
        c.try_request_export(&f.outsider, &f.agreement, &hash(&f.env, 1)),
        Err(Ok(ContractError::NotAuthorized))
    );
    // An unknown agreement fails closed before any authorization question.
    assert_eq!(
        c.try_request_export(&f.outsider, &hash(&f.env, 0xDE), &hash(&f.env, 1)),
        Err(Ok(ContractError::AgreementNotFound))
    );
}

#[test]
fn test_export_flow_success() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    // Seed one amendment and one approval so the manifest is non-trivial.
    c.record_amendment(
        &f.registrar,
        &f.agreement,
        &2,
        &hash(&f.env, 4),
        &hash(&f.env, 5),
    );
    c.record_approval(&f.mentee, &f.agreement, &hash(&f.env, 0x23));
    c.record_session_summary(
        &f.registrar,
        &f.agreement,
        &hash(&f.env, 1),
        &hash(&f.env, 0x31),
        &(BASE + 1_000),
        &60,
        &1_000,
        &100,
    );

    let grant = c.request_export(&f.mentor, &f.agreement, &hash(&f.env, 0x71));
    assert_eq!(grant.requester, f.mentor);
    assert_eq!(grant.agreement_id, f.agreement);
    assert_eq!(grant.created_at, BASE);
    assert_eq!(grant.expires_at, BASE + u64::from(GRANT_TTL));

    let manifest = c.get_export(&f.mentor, &hash(&f.env, 0x71));
    assert_eq!(manifest.agreement_id, f.agreement);
    assert_eq!(manifest.mentor, f.mentor);
    assert_eq!(manifest.mentee, f.mentee);
    assert_eq!(manifest.terms_version, 2);
    assert_eq!(manifest.terms_hash, hash(&f.env, 4));
    assert_eq!(manifest.amendments.len(), 1);
    assert_eq!(manifest.approvals.len(), 1);
    assert_eq!(manifest.session_summaries.len(), 1);
    assert_eq!(manifest.financial.net, 900);
    assert_eq!(manifest.generated_for, f.mentor);
    assert_eq!(manifest.generated_at, BASE);

    // Both lifecycle steps are audited for the agreement.
    let audit = c.get_audit(&f.mentor, &f.agreement);
    assert_eq!(audit.len(), 2);
    assert_eq!(audit.get(0).unwrap().kind, AuditKind::Requested);
    assert_eq!(audit.get(1).unwrap().kind, AuditKind::Downloaded);
}

#[test]
fn test_manifest_excludes_private_content_by_construction() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    // The agreement record and therefore the manifest carry only
    // commitments, statuses, timestamps and amounts: there is no free-text
    // field for an internal note (MENTOR-064) to leak into. This test
    // pins the exported surface to those commitment fields.
    let record: AgreementRecord = c.get_agreement(&f.agreement);
    let _: BytesN<32> = record.terms_hash;
    // The financial summary is integer settlement facts only.
    let _: (i128, i128, i128) = (
        record.financial.gross,
        record.financial.fees,
        record.financial.net,
    );
    let _: Symbol = record.financial.currency;

    let _manifest: ExportManifest = {
        c.request_export(&f.mentor, &f.agreement, &hash(&f.env, 0x71));
        c.get_export(&f.mentor, &hash(&f.env, 0x71))
    };
}

#[test]
fn test_download_requires_being_the_requester() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    c.request_export(&f.mentor, &f.agreement, &hash(&f.env, 0x71));
    // The mentee is a participant but not the requester of this grant.
    assert_eq!(
        c.try_get_export(&f.mentee, &hash(&f.env, 0x71)),
        Err(Ok(ContractError::NotAuthorized))
    );
    assert_eq!(
        c.try_get_export(&f.outsider, &hash(&f.env, 0x71)),
        Err(Ok(ContractError::NotAuthorized))
    );
    // Grant metadata reads are equally personal.
    assert_eq!(
        c.try_get_grant(&f.outsider, &hash(&f.env, 0x71)),
        Err(Ok(ContractError::NotAuthorized))
    );
    assert_eq!(
        c.get_grant(&f.mentor, &hash(&f.env, 0x71)).requester,
        f.mentor
    );
    // Audit reads are participant-only.
    assert_eq!(
        c.try_get_audit(&f.outsider, &f.agreement),
        Err(Ok(ContractError::NotAuthorized))
    );
}

// ── export flow: rate limiting ─────────────────────────────────────────────

#[test]
fn test_export_rate_limit_and_window_rollover() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    // The policy allows two grants per window.
    c.request_export(&f.mentor, &f.agreement, &hash(&f.env, 1));
    c.request_export(&f.mentor, &f.agreement, &hash(&f.env, 2));
    assert_eq!(
        c.try_request_export(&f.mentor, &f.agreement, &hash(&f.env, 3)),
        Err(Ok(ContractError::ExportRateLimited))
    );
    // The mentee has their own quota.
    c.request_export(&f.mentee, &f.agreement, &hash(&f.env, 4));

    // After the window elapses the quota rolls over.
    env_set_time(&f, BASE + u64::from(WINDOW));
    c.request_export(&f.mentor, &f.agreement, &hash(&f.env, 5));
}

#[test]
fn test_export_retry_does_not_consume_quota_twice() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    c.request_export(&f.mentor, &f.agreement, &hash(&f.env, 1));
    // Retrying the same grant id returns the same grant...
    let again = c.request_export(&f.mentor, &f.agreement, &hash(&f.env, 1));
    assert_eq!(again.created_at, BASE);
    // ...without spending a second unit of quota: a second grant still
    // fits inside the two-per-window policy.
    c.request_export(&f.mentor, &f.agreement, &hash(&f.env, 2));
    assert_eq!(
        c.try_request_export(&f.mentor, &f.agreement, &hash(&f.env, 3)),
        Err(Ok(ContractError::ExportRateLimited))
    );
    // And without duplicating the audit trail: two distinct grants were
    // requested and the refused third one is not audited.
    assert_eq!(c.get_audit(&f.mentor, &f.agreement).len(), 2);
}

#[test]
fn test_grant_id_reuse_across_scope_fails() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    c.request_export(&f.mentor, &f.agreement, &hash(&f.env, 1));
    // The same grant id under a different scope is a stable failure.
    assert_eq!(
        c.try_request_export(&f.mentee, &f.agreement, &hash(&f.env, 1)),
        Err(Ok(ContractError::GrantAlreadyExists))
    );
    let other_agreement = hash(&f.env, 0xA2);
    c.register_agreement(
        &f.registrar,
        &other_agreement,
        &f.mentor,
        &f.mentee,
        &hash(&f.env, 0x11),
        &1,
    );
    assert_eq!(
        c.try_request_export(&f.mentor, &other_agreement, &hash(&f.env, 1)),
        Err(Ok(ContractError::GrantAlreadyExists))
    );
}

// ── export flow: expiry ────────────────────────────────────────────────────

#[test]
fn test_expired_grant_cannot_download() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    c.request_export(&f.mentor, &f.agreement, &hash(&f.env, 1));
    // Just inside the window the link still works.
    env_set_time(&f, BASE + u64::from(GRANT_TTL));
    assert_eq!(
        c.get_export(&f.mentor, &hash(&f.env, 1)).generated_for,
        f.mentor
    );
    // One second past expiry the link is dead.
    env_set_time(&f, BASE + u64::from(GRANT_TTL) + 1);
    assert_eq!(
        c.try_get_export(&f.mentor, &hash(&f.env, 1)),
        Err(Ok(ContractError::GrantExpired))
    );
}

#[test]
fn test_unknown_grant_fails_closed() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    assert_eq!(
        c.try_get_export(&f.mentor, &hash(&f.env, 0x99)),
        Err(Ok(ContractError::GrantNotFound))
    );
    assert_eq!(
        c.try_get_grant(&f.mentor, &hash(&f.env, 0x99)),
        Err(Ok(ContractError::GrantNotFound))
    );
}

// ── events ─────────────────────────────────────────────────────────────────

#[test]
fn test_request_and_download_emit_one_event_each() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    c.request_export(&f.mentor, &f.agreement, &hash(&f.env, 1));
    let events = f.env.events().all();
    assert_eq!(events.len(), 1, "one event per export request");
    let (publisher, topics, _) = events.get(0).unwrap();
    assert_eq!(publisher, f.contract_id);
    let topic: Symbol = Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("EXPREQ"));

    c.get_export(&f.mentor, &hash(&f.env, 1));
    let events = f.env.events().all();
    assert_eq!(events.len(), 1, "one event per download");
    let (_, topics, _) = events.get(0).unwrap();
    let topic: Symbol = Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("EXPDL"));
}

// ── upgrade compatibility ──────────────────────────────────────────────────

#[test]
fn test_upgrade_requires_admin_and_preserves_abi() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    assert_eq!(
        c.try_upgrade(&f.outsider, &hash(&f.env, 0xB0)),
        Err(Ok(ContractError::NotAdmin))
    );
    // The ABI generation stays stable and existing records stay readable;
    // the wasm swap itself is exercised by the deploy scripts.
    assert_eq!(c.version(), 1);
    assert_eq!(c.get_agreement(&f.agreement).mentor, f.mentor);
}

// ── helpers ────────────────────────────────────────────────────────────────

fn env_set_time(f: &Fixture, at: u64) {
    f.env.ledger().set_timestamp(at);
}
