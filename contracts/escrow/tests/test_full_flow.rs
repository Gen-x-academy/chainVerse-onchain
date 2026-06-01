#![cfg(test)]

extern crate soroban_sdk;
use crate::{EscrowContract, EscrowContractClient, EscrowError};
use soroban_sdk::{Env, String};

#[test]
use soroban_sdk::{Address, Env, String};

#[test]
fn e2e_escrow_version_and_count_baseline() {

use crate::{EscrowContract, EscrowContractClient, EscrowError};
use soroban_sdk::{Address, Env, String};

#[test]
fn e2e_escrow_version_and_dispute_baseline() {
    let env = Env::default();
    let contract_id = env.register_contract(None, EscrowContract);
    let client = EscrowContractClient::new(&env, &contract_id);

    assert_eq!(client.version(), String::from_str(&env, "1.0.0"));
    assert_eq!(
        client.try_resolve_dispute(&1, &true),
        Err(Ok(EscrowError::DisputeResolutionNotImplemented))
    );
    assert_eq!(client.get_escrow_count(), 0u64);
}

#[test]
fn e2e_escrow_count_starts_at_zero() {
    let env = Env::default();
    let contract_id = env.register_contract(None, EscrowContract);
    let client = EscrowContractClient::new(&env, &contract_id);

    assert_eq!(client.get_escrow_count(), 0);
    assert_eq!(client.get_total_volume(), 0);
}
