#![no_std]

use soroban_sdk::{contract, contractimpl, Env, Symbol};

#[contract]
pub struct ChainverseCore;

#[contractimpl]
impl ChainverseCore {
    pub fn hello(env: Env, to: Symbol) -> Symbol {
        env.logger().log(&to);
        to
    }
}


#[contractimpl]
impl CourseContract {

    pub fn purchase_course(
        env: Env,
        buyer: Address,
        course_id: u64,
    ) {
        buyer.require_auth();

        let key = DataKey::Purchase(buyer.clone(), course_id);

        // Check if already purchased
        if env.storage().instance().has(&key) {
            panic_with_error!(&env, ContractError::AlreadyPurchased);
        }

        // TODO: Add token transfer logic here

        // Save purchase record
        env.storage().instance().set(&key, &true);
    }
}