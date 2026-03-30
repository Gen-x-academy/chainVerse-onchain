use soroban_sdk::{contract, contractimpl, Env, Address};

mod admin;
mod storage;

#[contract]
pub struct TokenContract;

#[contractimpl]
impl TokenContract {

    pub fn initialize(env: Env, admin: Address) {
        if storage::has_admin(&env) {
            panic!("Already initialized");
        }
        admin.require_auth();
        storage::set_admin(&env, &admin);
    }

    pub fn mint(env: Env, to: Address, amount: i128) {
        admin::require_admin(&env);

        // checked arithmetic
        let balance = storage::balance_of(&env, &to);
        let new_balance = balance.checked_add(amount)
            .expect("Overflow");

        storage::set_balance(&env, &to, &new_balance);
    }
}
