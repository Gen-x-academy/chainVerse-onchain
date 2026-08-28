use crate::LibraryRightsContract;
use soroban_sdk::{Address, Env, String};

mod content;
mod governance;
mod metadata;
mod privacy;
mod registry;
mod storage;

/// Shared test setup: a fresh env with a freshly registered contract.
fn setup() -> (Env, Address) {
    let env = Env::default();
    let contract_id = env.register(LibraryRightsContract, ());
    (env, contract_id)
}

#[test]
fn test_version_reports_current_abi() {
    let (env, contract_id) = setup();
    let client = crate::LibraryRightsContractClient::new(&env, &contract_id);

    assert_eq!(client.version(), String::from_str(&env, "0.6.0"));
}
