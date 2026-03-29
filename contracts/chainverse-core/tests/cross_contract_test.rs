#![allow(clippy::unwrap_used, clippy::expect_used)]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, vec, IntoVal,
    testutils::Address as _, panic_with_error, Address, Env, Error as SorobanError,
};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OracleMode {
    Allow,
    Deny,
    Malformed,
    Fail,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalComplianceResponse {
    decision_code: u32,
    rules_checked: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComplianceDecision {
    allowed: bool,
    external_rules_checked: u32,
}

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum MockOracleError {
    ServiceUnavailable = 1,
}

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ComplianceHarnessError {
    AccessDenied = 1,
    InvalidExternalResponse = 2,
    ExternalCallFailed = 3,
}

#[contract]
pub struct MockComplianceOracle;

const ORACLE_MODE_KEY: u32 = 1;

#[contractimpl]
impl MockComplianceOracle {
    pub fn set_mode(env: Env, mode: OracleMode) {
        env.storage().instance().set(&ORACLE_MODE_KEY, &mode);
    }

    pub fn check(
        env: Env,
        _actor: Address,
        _resource: Address,
    ) -> Result<ExternalComplianceResponse, MockOracleError> {
        let mode: OracleMode = env
            .storage()
            .instance()
            .get(&ORACLE_MODE_KEY)
            .unwrap_or(OracleMode::Allow);

        match mode {
            OracleMode::Allow => Ok(ExternalComplianceResponse {
                decision_code: 1,
                rules_checked: 4,
            }),
            OracleMode::Deny => Ok(ExternalComplianceResponse {
                decision_code: 0,
                rules_checked: 4,
            }),
            OracleMode::Malformed => Ok(ExternalComplianceResponse {
                decision_code: 7,
                rules_checked: 4,
            }),
            OracleMode::Fail => Err(MockOracleError::ServiceUnavailable),
        }
    }
}

#[contract]
pub struct ComplianceCrossContractHarness;

#[contractimpl]
impl ComplianceCrossContractHarness {
    /// Verifies the invariant that the compliance entrypoint only accepts
    /// well-formed allow/deny responses from an external policy oracle.
    pub fn verify_external_call(
        env: Env,
        oracle: Address,
        actor: Address,
        resource: Address,
    ) -> ComplianceDecision {
        actor.require_auth();

        let response = match env.try_invoke_contract::<ExternalComplianceResponse, MockOracleError>(
            &oracle,
            &symbol_short!("check"),
            vec![&env, actor.clone().into_val(&env), resource.into_val(&env)],
        ) {
            Ok(Ok(response)) => response,
            Ok(Err(_)) => panic_with_error!(&env, ComplianceHarnessError::InvalidExternalResponse),
            Err(Ok(_)) | Err(Err(_)) => {
                panic_with_error!(&env, ComplianceHarnessError::ExternalCallFailed)
            }
        };

        match response.decision_code {
            1 => ComplianceDecision {
                allowed: true,
                external_rules_checked: response.rules_checked,
            },
            0 => panic_with_error!(&env, ComplianceHarnessError::AccessDenied),
            _ => panic_with_error!(&env, ComplianceHarnessError::InvalidExternalResponse),
        }
    }
}

fn setup() -> (
    Env,
    ComplianceCrossContractHarnessClient<'static>,
    MockComplianceOracleClient<'static>,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let compliance_id = env.register(ComplianceCrossContractHarness, ());
    let oracle_id = env.register(MockComplianceOracle, ());

    let compliance = ComplianceCrossContractHarnessClient::new(&env, &compliance_id);
    let oracle = MockComplianceOracleClient::new(&env, &oracle_id);
    let actor = Address::generate(&env);
    let resource = Address::generate(&env);

    (env, compliance, oracle, oracle_id, actor, resource)
}

#[test]
fn test_cross_contract_allow_response_is_parsed_correctly() {
    let (_env, compliance, oracle, oracle_id, actor, resource) = setup();
    oracle.set_mode(&OracleMode::Allow);

    let decision = compliance.verify_external_call(&oracle_id, &actor, &resource);

    assert_eq!(
        decision,
        ComplianceDecision {
            allowed: true,
            external_rules_checked: 4,
        }
    );
}

#[test]
fn test_cross_contract_deny_response_blocks_the_primary_contract() {
    let (_env, compliance, oracle, oracle_id, actor, resource) = setup();
    oracle.set_mode(&OracleMode::Deny);

    assert_eq!(
        compliance.try_verify_external_call(&oracle_id, &actor, &resource),
        Err(Ok(SorobanError::from_contract_error(
            ComplianceHarnessError::AccessDenied as u32,
        )))
    );
}

#[test]
fn test_cross_contract_malformed_response_is_rejected() {
    let (_env, compliance, oracle, oracle_id, actor, resource) = setup();
    oracle.set_mode(&OracleMode::Malformed);

    assert_eq!(
        compliance.try_verify_external_call(&oracle_id, &actor, &resource),
        Err(Ok(SorobanError::from_contract_error(
            ComplianceHarnessError::InvalidExternalResponse as u32,
        )))
    );
}

#[test]
fn test_cross_contract_external_failure_is_mapped_gracefully() {
    let (_env, compliance, oracle, oracle_id, actor, resource) = setup();
    oracle.set_mode(&OracleMode::Fail);

    assert_eq!(
        compliance.try_verify_external_call(&oracle_id, &actor, &resource),
        Err(Ok(SorobanError::from_contract_error(
            ComplianceHarnessError::ExternalCallFailed as u32,
        )))
    );
}
