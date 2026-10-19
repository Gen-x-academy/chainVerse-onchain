use soroban_sdk::{symbol_short, Address, BytesN, Env};

use crate::types::{AgreementStatus, AmendmentStatus};

/// Event: Agreement created
pub fn agreement_created(
    env: &Env,
    agreement_id: u64,
    mentor: &Address,
    mentee: &Address,
    start_time: u64,
    end_time: u64,
) {
    env.events().publish(
        (symbol_short!("agmt_new"),),
        (agreement_id, mentor, mentee, start_time, end_time),
    );
}

/// Event: Agreement accepted by a party
pub fn agreement_accepted(
    env: &Env,
    agreement_id: u64,
    actor: &Address,
    agreement_hash: &BytesN<32>,
    nonce: u64,
) {
    env.events().publish(
        (symbol_short!("agmt_acpt"),),
        (agreement_id, actor, agreement_hash, nonce),
    );
}

/// Event: Agreement status changed
pub fn agreement_status_changed(
    env: &Env,
    agreement_id: u64,
    old_status: AgreementStatus,
    new_status: AgreementStatus,
) {
    env.events().publish(
        (symbol_short!("agmt_stat"),),
        (agreement_id, old_status as u32, new_status as u32),
    );
}

/// Event: Amendment proposed
pub fn amendment_proposed(
    env: &Env,
    amendment_id: u64,
    agreement_id: u64,
    proposer: &Address,
    impact_hash: &BytesN<32>,
) {
    env.events().publish(
        (symbol_short!("amnd_prop"),),
        (amendment_id, agreement_id, proposer, impact_hash),
    );
}

/// Event: Amendment approved by a party
pub fn amendment_approved(
    env: &Env,
    amendment_id: u64,
    approver: &Address,
    is_fully_approved: bool,
) {
    env.events().publish(
        (symbol_short!("amnd_aprv"),),
        (amendment_id, approver, is_fully_approved),
    );
}

/// Event: Amendment rejected
pub fn amendment_rejected(env: &Env, amendment_id: u64, rejector: &Address) {
    env.events().publish(
        (symbol_short!("amnd_rjct"),),
        (amendment_id, rejector),
    );
}

/// Event: Amendment activated
pub fn amendment_activated(env: &Env, amendment_id: u64, agreement_id: u64, activated_at: u64) {
    env.events().publish(
        (symbol_short!("amnd_actv"),),
        (amendment_id, agreement_id, activated_at),
    );
}

/// Event: User blocked another user
pub fn user_blocked(env: &Env, blocker: &Address, blocked: &Address, blocked_at: u64) {
    env.events().publish(
        (symbol_short!("usr_block"),),
        (blocker, blocked, blocked_at),
    );
}

/// Event: User unblocked another user
pub fn user_unblocked(env: &Env, blocker: &Address, blocked: &Address) {
    env.events().publish(
        (symbol_short!("usr_unblk"),),
        (blocker, blocked),
    );
}

/// Event: Contract initialized
pub fn initialized(env: &Env, admin: &Address) {
    env.events().publish((symbol_short!("init"),), (admin,));
}
