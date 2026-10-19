use soroban_sdk::{symbol_short, Address, BytesN, Env};

pub fn mentorship_proposed(
    env: &Env,
    match_id: &BytesN<32>,
    mentor: &Address,
    mentee: &Address,
    program_id: &BytesN<32>,
    start_date: u64,
    end_date: u64,
    amount: i128,
) {
    env.events().publish(
        (symbol_short!("MNT_PRO"),),
        (
            match_id.clone(),
            mentor.clone(),
            mentee.clone(),
            program_id.clone(),
            start_date,
            end_date,
            amount,
        ),
    );
}

pub fn mentee_accepted(env: &Env, match_id: &BytesN<32>, mentee: &Address, accepted_at: u64) {
    env.events().publish(
        (symbol_short!("MNT_MAC"),),
        (match_id.clone(), mentee.clone(), accepted_at),
    );
}

pub fn mentor_accepted(env: &Env, match_id: &BytesN<32>, mentor: &Address, accepted_at: u64) {
    env.events().publish(
        (symbol_short!("MNT_MEC"),),
        (match_id.clone(), mentor.clone(), accepted_at),
    );
}

pub fn mentorship_activated(
    env: &Env,
    match_id: &BytesN<32>,
    mentor: &Address,
    mentee: &Address,
    activated_at: u64,
) {
    env.events().publish(
        (symbol_short!("MNT_ACT"),),
        (match_id.clone(), mentor.clone(), mentee.clone(), activated_at),
    );
}

pub fn mentorship_rejected(
    env: &Env,
    match_id: &BytesN<32>,
    rejecting_party: &Address,
    reason_code: u32,
) {
    env.events().publish(
        (symbol_short!("MNT_REJ"),),
        (match_id.clone(), rejecting_party.clone(), reason_code),
    );
}

pub fn mentorship_terminated(
    env: &Env,
    match_id: &BytesN<32>,
    mentor: &Address,
    mentee: &Address,
    reason_code: u32,
    recovery_amount: i128,
) {
    env.events().publish(
        (symbol_short!("MNT_TRM"),),
        (
            match_id.clone(),
            mentor.clone(),
            mentee.clone(),
            reason_code,
            recovery_amount,
        ),
    );
}

pub fn mentorship_completed(
    env: &Env,
    match_id: &BytesN<32>,
    mentor: &Address,
    mentee: &Address,
    completed_at: u64,
    final_amount: i128,
) {
    env.events().publish(
        (symbol_short!("MNT_CMP"),),
        (
            match_id.clone(),
            mentor.clone(),
            mentee.clone(),
            completed_at,
            final_amount,
        ),
    );
}
