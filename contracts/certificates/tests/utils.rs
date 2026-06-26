use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env};
pub fn random_address(env: &Env) -> Address {
    Address::generate(env)
}
