#![no_std]

use soroban_sdk::{Env, Symbol, symbol_short};

pub struct EventEmitter;

impl EventEmitter {

    // Course Purchased Event
    pub fn course_purchased(
        env: &Env,
        buyer: &soroban_sdk::Address,
        course_id: u64,
        amount: i128,
    ) {
        let topic = (
            symbol_short!("chainverse"),
            symbol_short!("course"),
            symbol_short!("purchased"),
        );

        env.events().publish(
            topic,
            (buyer, course_id, amount),
        );
    }

    // Reward Claimed Event
    pub fn reward_claimed(
        env: &Env,
        user: &soroban_sdk::Address,
        reward_id: u64,
        amount: i128,
    ) {
        let topic = (
            symbol_short!("chainverse"),
            symbol_short!("reward"),
            symbol_short!("claimed"),
        );

        env.events().publish(
            topic,
            (user, reward_id, amount),
        );
    }

    // Certificate Minted Event
    pub fn certificate_minted(
        env: &Env,
        user: &soroban_sdk::Address,
        course_id: u64,
        token_id: u64,
    ) {
        let topic = (
            symbol_short!("chainverse"),
            symbol_short!("certificate"),
            symbol_short!("minted"),
        );

        env.events().publish(
            topic,
            (user, course_id, token_id),
        );
    }
}