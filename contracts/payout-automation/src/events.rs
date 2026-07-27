use soroban_sdk::{symbol_short, Address, Env};

/// Emitted when a batch of payouts is successfully executed.
pub fn emit_batch_executed(env: &Env, caller: &Address, count: u32, total: i128) {
    env.events().publish(
        (symbol_short!("batch_exe"), caller.clone()),
        (count, total),
    );
}

/// Emitted when a single payout within a batch is sent.
pub fn emit_payout_sent(env: &Env, recipient: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("payout"), recipient.clone()),
        amount,
    );
}

/// Emitted when a student pays for a course.
pub fn emit_course_paid(env: &Env, student: &Address, course_id: u64, amount: i128) {
    env.events().publish(
        (symbol_short!("crs_paid"), student.clone()),
        (course_id, amount),
    );
}
