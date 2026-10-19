#![no_std]

#[cfg(test)]
mod tests;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    MentorshipAlreadyExists = 4,
    MentorshipNotFound = 5,
    Unauthorized = 6,
    InvalidTransition = 7,
    InvalidParticipants = 8,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    Mentorship(BytesN<32>),
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MentorshipStatus {
    Draft,
    Published,
    Accepting,
    Matched,
    Active,
    Paused,
    Completed,
    Cancelled,
    Disputed,
    Archived,
}

/// Public state only. Conversation text, schedules, and evidence belong off-chain.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mentorship {
    pub id: BytesN<32>,
    pub mentor: Address,
    pub mentee: Option<Address>,
    pub commitment_hash: BytesN<32>,
    pub status: MentorshipStatus,
    pub created_at: u64,
    pub updated_at: u64,
}

#[contract]
pub struct MentorshipLifecycle;

#[contractimpl]
impl MentorshipLifecycle {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        bump_instance(&env);
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        bump_instance(&env);
        env.events()
            .publish((symbol_short!("MTR_INIT"),), (admin,));
        Ok(())
    }

    /// Create a draft. The mentor is the creator; the mentee is attached only
    /// when an accepting opportunity is matched. The commitment is immutable.
    pub fn create_mentorship(
        env: Env,
        mentor: Address,
        id: BytesN<32>,
        commitment_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        if !env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::NotInitialized);
        }
        mentor.require_auth();
        let key = DataKey::Mentorship(id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::MentorshipAlreadyExists);
        }
        let now = env.ledger().timestamp();
        let record = Mentorship {
            id: id.clone(),
            mentor: mentor.clone(),
            mentee: None,
            commitment_hash,
            status: MentorshipStatus::Draft,
            created_at: now,
            updated_at: now,
        };
        save_record(&env, &record);
        env.events()
            .publish((symbol_short!("MTR_NEW"),), (id, mentor, now));
        Ok(())
    }

    /// Assign a mentee and move an accepting opportunity to Matched. Both
    /// parties authorize the match; neither party's private content is stored.
    pub fn match_mentorship(
        env: Env,
        caller: Address,
        id: BytesN<32>,
        mentee: Address,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        let mut record = get_record(&env, &id)?;
        if record.status != MentorshipStatus::Accepting {
            return Err(ContractError::InvalidTransition);
        }
        if caller != record.mentor {
            return Err(ContractError::Unauthorized);
        }
        if mentee == record.mentor {
            return Err(ContractError::InvalidParticipants);
        }
        caller.require_auth();
        mentee.require_auth();

        record.mentee = Some(mentee.clone());
        record.status = MentorshipStatus::Matched;
        record.updated_at = env.ledger().timestamp();
        save_record(&env, &record);
        env.events().publish(
            (symbol_short!("MTR_MATC"),),
            (id, record.mentor, mentee, record.updated_at),
        );
        Ok(())
    }

    /// Apply a guarded lifecycle transition. Dispute resolution and archival
    /// are admin-only; activation/completion require both parties.
    pub fn transition(
        env: Env,
        caller: Address,
        id: BytesN<32>,
        next: MentorshipStatus,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        let mut record = get_record(&env, &id)?;
        let previous = record.status;
        authorize_transition(&env, &record, &caller, previous, next)?;

        record.status = next;
        record.updated_at = env.ledger().timestamp();
        save_record(&env, &record);
        env.events().publish(
            (symbol_short!("MTR_STEP"),),
            (id, previous, next, caller, record.updated_at),
        );
        Ok(())
    }

    pub fn get_mentorship(env: Env, id: BytesN<32>) -> Result<Mentorship, ContractError> {
        bump_instance(&env);
        get_record(&env, &id)
    }
}

fn authorize_transition(
    env: &Env,
    record: &Mentorship,
    caller: &Address,
    previous: MentorshipStatus,
    next: MentorshipStatus,
) -> Result<(), ContractError> {
    use MentorshipStatus as S;

    match (previous, next) {
        (S::Draft, S::Published)
        | (S::Published, S::Accepting)
        | (S::Draft, S::Cancelled)
        | (S::Published, S::Cancelled)
        | (S::Accepting, S::Cancelled)
        | (S::Matched, S::Cancelled) => require_mentor(record, caller),
        (S::Matched, S::Active) | (S::Paused, S::Active) | (S::Active, S::Completed) => {
            require_both_parties(record, caller)
        }
        (S::Active, S::Paused) | (S::Active, S::Disputed) | (S::Paused, S::Disputed)
        | (S::Completed, S::Disputed) | (S::Active, S::Cancelled) | (S::Paused, S::Cancelled) => {
            require_party(record, caller)
        }
        (S::Disputed, S::Active) | (S::Disputed, S::Completed) | (S::Disputed, S::Cancelled)
        | (S::Completed, S::Archived) | (S::Cancelled, S::Archived) => {
            require_admin(env, caller)
        }
        _ => Err(ContractError::InvalidTransition),
    }
}

fn require_mentor(record: &Mentorship, caller: &Address) -> Result<(), ContractError> {
    if caller != &record.mentor {
        return Err(ContractError::Unauthorized);
    }
    caller.require_auth();
    Ok(())
}

fn require_party(record: &Mentorship, caller: &Address) -> Result<(), ContractError> {
    if caller != &record.mentor && record.mentee.as_ref() != Some(caller) {
        return Err(ContractError::Unauthorized);
    }
    caller.require_auth();
    Ok(())
}

fn require_both_parties(record: &Mentorship, caller: &Address) -> Result<(), ContractError> {
    if caller != &record.mentor && record.mentee.as_ref() != Some(caller) {
        return Err(ContractError::Unauthorized);
    }
    let mentee = record
        .mentee
        .as_ref()
        .ok_or(ContractError::InvalidParticipants)?;
    record.mentor.require_auth();
    mentee.require_auth();
    Ok(())
}

fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(ContractError::NotInitialized)?;
    if caller != &admin {
        return Err(ContractError::NotAdmin);
    }
    caller.require_auth();
    Ok(())
}

fn get_record(env: &Env, id: &BytesN<32>) -> Result<Mentorship, ContractError> {
    let key = DataKey::Mentorship(id.clone());
    let record = env
        .storage()
        .persistent()
        .get(&key)
        .ok_or(ContractError::MentorshipNotFound)?;
    env.storage()
        .persistent()
        .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    Ok(record)
}

fn save_record(env: &Env, record: &Mentorship) {
    let key = DataKey::Mentorship(record.id.clone());
    env.storage().persistent().set(&key, record);
    env.storage()
        .persistent()
        .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
}

fn bump_instance(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(RECORD_MIN_TTL, RECORD_MAX_TTL);
}