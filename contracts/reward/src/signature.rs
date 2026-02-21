use soroban_sdk::{Env, Address, Bytes, BytesN};

pub fn build_message(
    env: &Env,
    user: &Address,
    course_id: u32,
    amount: i128,
    nonce: &BytesN<32>,
    expires_at: u64,
) -> Bytes {
    let mut msg = Bytes::new(env);

    msg.append(&Bytes::from_slice(env, b"CHAINVERSE_REWARD:"));
    msg.append(&user.serialize(env));
    msg.append(&course_id.into());
    msg.append(&amount.into());
    msg.append(nonce);
    msg.append(&expires_at.into());

    msg
}