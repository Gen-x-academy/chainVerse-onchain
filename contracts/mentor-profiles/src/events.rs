use soroban_sdk::{Address, Bytes, Env};

/// Emitted when a mentor publishes their profile
pub fn emit_profile_published(env: &Env, mentor_id: &Address, expertise_hash: &Bytes, rate_per_hour: u128) {
    env.events().publish(
        ("mentor", "profile_published"),
        (mentor_id, expertise_hash, rate_per_hour),
    );
}

/// Emitted when mentor availability is updated
pub fn emit_availability_updated(env: &Env, mentor_id: &Address, status: u32) {
    env.events().publish(
        ("mentor", "availability_updated"),
        (mentor_id, status),
    );
}

/// Emitted when a mentor's rating is recorded
pub fn emit_rating_updated(env: &Env, mentor_id: &Address, new_rating_avg: u32, rating_count: u32) {
    env.events().publish(
        ("mentor", "rating_updated"),
        (mentor_id, new_rating_avg, rating_count),
    );
}

/// Emitted when a mentor's profile is archived
pub fn emit_profile_archived(env: &Env, mentor_id: &Address) {
    env.events().publish(
        ("mentor", "profile_archived"),
        (mentor_id,),
    );
}

/// Emitted when a mentor's profile is restored from archived status
pub fn emit_profile_restored(env: &Env, mentor_id: &Address) {
    env.events().publish(
        ("mentor", "profile_restored"),
        (mentor_id,),
    );
}

/// Emitted when reputation score is updated
pub fn emit_reputation_updated(env: &Env, mentor_id: &Address, new_score: u32) {
    env.events().publish(
        ("mentor", "reputation_updated"),
        (mentor_id, new_score),
    );
}
