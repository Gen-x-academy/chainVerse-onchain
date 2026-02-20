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