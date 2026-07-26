#![no_std]
use soroban_sdk::{contract, contractimpl, symbol_short, Address, Env};

#[contract]
pub struct ChainVerseContract;

#[contractimpl]
impl ChainVerseContract {
    pub fn toggle_pause(env: Env, wallet: Address, course_id: u64) {
        env.events().publish(
            (symbol_short!("CERT_REV"),),
            (wallet.clone(), course_id.clone())
        );
    }

    pub fn revoke_certificate(env: Env, wallet: Address, course_id: u64) {
        env.events().publish(
            (symbol_short!("CERT_REV"),),
            (wallet.clone(), course_id.clone())
        );
    }
}
