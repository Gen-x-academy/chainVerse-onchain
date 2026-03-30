#![no_std]

use soroban_sdk::{contract, contractimpl, symbol, Env};

#[contract]
pub struct Complimenter;

#[contractimpl]
impl Complimenter {
    pub fn compliment(env: Env, name: String) -> String {
        let compliments = [
            "You're awesome!",
            "Great job!",
            "Keep up the good work!",
            "You're a star!",
            "Fantastic effort!",
        ];
        let idx = (env.block().number() as usize) % compliments.len();
        format!("{}, {}", name, compliments[idx])
    }
}
