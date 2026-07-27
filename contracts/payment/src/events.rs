use soroban_sdk::{symbol_short, Address, Env, Symbol};

pub fn payment_recorded(
    env: &Env,
    student: Address,
    course_id: Symbol,
    amount: i128,
    instructor: Address,
) {
    env.events().publish(
        (symbol_short!("PYMT_RCD"),),
        (student, course_id, amount, instructor),
    );
}

pub fn refund_issued(env: &Env, student: Address, course_id: Symbol, amount: i128) {
    env.events()
        .publish((symbol_short!("RFND_ISS"),), (student, course_id, amount));
}

pub fn fee_set(env: &Env, fee_percent: u32) {
    env.events()
        .publish((symbol_short!("FEE_SET"),), (fee_percent,));
}

pub fn withdrawal_processed(env: &Env, instructor: Address, amount: i128) {
    env.events()
        .publish((symbol_short!("WTHDW"),), (instructor, amount));
}
