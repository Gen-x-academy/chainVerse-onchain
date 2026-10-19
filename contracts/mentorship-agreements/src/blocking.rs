use soroban_sdk::{Address, BytesN, Env};

use crate::{
    errors::MentorshipError,
    events,
    storage::{get_block, remove_block, set_block},
    types::{Block, MatchingAllowed},
};

/// Block a user from matching (MENTOR-044)
pub fn block_user(
    env: &Env,
    blocker: Address,
    blocked: Address,
    reason_hash: Option<BytesN<32>>,
) -> Result<(), MentorshipError> {
    // Authorization: require blocker to authenticate
    blocker.require_auth();

    // Validation: cannot block self
    if blocker == blocked {
        return Err(MentorshipError::CannotBlockSelf);
    }

    // Check if block already exists
    if get_block(env, &blocker, &blocked).is_some() {
        return Err(MentorshipError::BlockAlreadyExists);
    }

    // Create block record
    let block = Block {
        blocker: blocker.clone(),
        blocked: blocked.clone(),
        blocked_at: env.ledger().timestamp(),
        reason_hash,
    };

    // Store block
    set_block(env, &block);

    // Emit event
    events::user_blocked(env, &blocker, &blocked, block.blocked_at);

    Ok(())
}

/// Unblock a user
pub fn unblock_user(
    env: &Env,
    blocker: Address,
    blocked: Address,
) -> Result<(), MentorshipError> {
    // Authorization: require blocker to authenticate
    blocker.require_auth();

    // Validation: cannot unblock self
    if blocker == blocked {
        return Err(MentorshipError::CannotBlockSelf);
    }

    // Check if block exists
    if get_block(env, &blocker, &blocked).is_none() {
        return Err(MentorshipError::BlockNotFound);
    }

    // Remove block
    remove_block(env, &blocker, &blocked);

    // Emit event
    events::user_unblocked(env, &blocker, &blocked);

    Ok(())
}

/// Check if two users can be matched (bidirectional block check)
pub fn check_matching_allowed(
    env: &Env,
    user1: &Address,
    user2: &Address,
) -> MatchingAllowed {
    let user1_blocks_user2 = get_block(env, user1, user2).is_some();
    let user2_blocks_user1 = get_block(env, user2, user1).is_some();

    match (user1_blocks_user2, user2_blocks_user1) {
        (false, false) => MatchingAllowed::Allowed,
        (true, false) => MatchingAllowed::BlockedByFirst,
        (false, true) => MatchingAllowed::BlockedBySecond,
        (true, true) => MatchingAllowed::MutualBlock,
    }
}

/// Get block details if exists
pub fn get_block_info(
    env: &Env,
    blocker: &Address,
    blocked: &Address,
) -> Option<Block> {
    get_block(env, blocker, blocked)
}
