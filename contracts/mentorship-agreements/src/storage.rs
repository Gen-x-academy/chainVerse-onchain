use soroban_sdk::{Address, Bytes, BytesN, Env};

use crate::{
    errors::MentorshipError,
    types::{Acceptance, Agreement, Amendment, Block},
};

/// Storage keys for the contract
#[derive(Clone)]
pub enum DataKey {
    /// Admin address
    Admin,
    /// Next agreement ID counter
    NextAgreementId,
    /// Next amendment ID counter
    NextAmendmentId,
    /// Agreement by ID: Agreement(u64) -> Agreement
    Agreement(u64),
    /// Acceptance record: Acceptance(agreement_id, actor) -> Acceptance
    AcceptanceRecord(u64, Address),
    /// Amendment by ID: Amendment(u64) -> Amendment
    Amendment(u64),
    /// Block relationship: Block(blocker, blocked) -> Block
    BlockRelation(Address, Address),
    /// User nonce for replay protection: UserNonce(address) -> u64
    UserNonce(Address),
    /// Initialized flag
    Initialized,
}

impl DataKey {
    pub fn to_key(&self, env: &Env) -> Bytes {
        match self {
            DataKey::Admin => {
                let mut key = Bytes::new(env);
                key.append(&Bytes::from_slice(env, b"Admin"));
                key
            }
            DataKey::NextAgreementId => {
                let mut key = Bytes::new(env);
                key.append(&Bytes::from_slice(env, b"NextAgmtId"));
                key
            }
            DataKey::NextAmendmentId => {
                let mut key = Bytes::new(env);
                key.append(&Bytes::from_slice(env, b"NextAmendId"));
                key
            }
            DataKey::Agreement(id) => {
                let mut key = Bytes::new(env);
                key.append(&Bytes::from_slice(env, b"Agmt"));
                key.append(&Bytes::from_array(env, &id.to_be_bytes()));
                key
            }
            DataKey::AcceptanceRecord(agreement_id, actor) => {
                let mut key = Bytes::new(env);
                key.append(&Bytes::from_slice(env, b"Accept"));
                key.append(&Bytes::from_array(env, &agreement_id.to_be_bytes()));
                key.append(&Bytes::from_slice(env, actor.to_string().as_bytes()));
                key
            }
            DataKey::Amendment(id) => {
                let mut key = Bytes::new(env);
                key.append(&Bytes::from_slice(env, b"Amend"));
                key.append(&Bytes::from_array(env, &id.to_be_bytes()));
                key
            }
            DataKey::BlockRelation(blocker, blocked) => {
                let mut key = Bytes::new(env);
                key.append(&Bytes::from_slice(env, b"Block"));
                key.append(&Bytes::from_slice(env, blocker.to_string().as_bytes()));
                key.append(&Bytes::from_slice(env, blocked.to_string().as_bytes()));
                key
            }
            DataKey::UserNonce(addr) => {
                let mut key = Bytes::new(env);
                key.append(&Bytes::from_slice(env, b"Nonce"));
                key.append(&Bytes::from_slice(env, addr.to_string().as_bytes()));
                key
            }
            DataKey::Initialized => {
                let mut key = Bytes::new(env);
                key.append(&Bytes::from_slice(env, b"Init"));
                key
            }
        }
    }
}

// Admin management
pub fn get_admin(env: &Env) -> Result<Address, MentorshipError> {
    let key = DataKey::Admin.to_key(env);
    env.storage()
        .instance()
        .get(&key)
        .ok_or(MentorshipError::NotInitialized)
}

pub fn set_admin(env: &Env, admin: &Address) {
    let key = DataKey::Admin.to_key(env);
    env.storage().instance().set(&key, admin);
}

pub fn require_admin(env: &Env, caller: &Address) -> Result<(), MentorshipError> {
    let admin = get_admin(env)?;
    if admin != *caller {
        return Err(MentorshipError::Unauthorized);
    }
    Ok(())
}

// Initialization
pub fn is_initialized(env: &Env) -> bool {
    let key = DataKey::Initialized.to_key(env);
    env.storage().instance().get(&key).unwrap_or(false)
}

pub fn set_initialized(env: &Env) {
    let key = DataKey::Initialized.to_key(env);
    env.storage().instance().set(&key, &true);
}

// Agreement ID management
pub fn get_next_agreement_id(env: &Env) -> u64 {
    let key = DataKey::NextAgreementId.to_key(env);
    env.storage().instance().get(&key).unwrap_or(1)
}

pub fn increment_agreement_id(env: &Env) -> u64 {
    let current = get_next_agreement_id(env);
    let next = current.checked_add(1).expect("Agreement ID overflow");
    let key = DataKey::NextAgreementId.to_key(env);
    env.storage().instance().set(&key, &next);
    current
}

// Amendment ID management
pub fn get_next_amendment_id(env: &Env) -> u64 {
    let key = DataKey::NextAmendmentId.to_key(env);
    env.storage().instance().get(&key).unwrap_or(1)
}

pub fn increment_amendment_id(env: &Env) -> u64 {
    let current = get_next_amendment_id(env);
    let next = current.checked_add(1).expect("Amendment ID overflow");
    let key = DataKey::NextAmendmentId.to_key(env);
    env.storage().instance().set(&key, &next);
    current
}

// Agreement storage
pub fn get_agreement(env: &Env, id: u64) -> Result<Agreement, MentorshipError> {
    let key = DataKey::Agreement(id).to_key(env);
    env.storage()
        .persistent()
        .get(&key)
        .ok_or(MentorshipError::AgreementNotFound)
}

pub fn set_agreement(env: &Env, agreement: &Agreement) {
    let key = DataKey::Agreement(agreement.id).to_key(env);
    env.storage().persistent().set(&key, agreement);
    env.storage().persistent().extend_ttl(&key, 5184000, 5184000); // ~60 days
}

// Acceptance storage
pub fn get_acceptance(env: &Env, agreement_id: u64, actor: &Address) -> Option<Acceptance> {
    let key = DataKey::AcceptanceRecord(agreement_id, actor.clone()).to_key(env);
    env.storage().persistent().get(&key)
}

pub fn set_acceptance(env: &Env, acceptance: &Acceptance) {
    let key = DataKey::AcceptanceRecord(acceptance.agreement_id, acceptance.actor.clone()).to_key(env);
    env.storage().persistent().set(&key, acceptance);
    env.storage().persistent().extend_ttl(&key, 5184000, 5184000);
}

// Amendment storage
pub fn get_amendment(env: &Env, id: u64) -> Result<Amendment, MentorshipError> {
    let key = DataKey::Amendment(id).to_key(env);
    env.storage()
        .persistent()
        .get(&key)
        .ok_or(MentorshipError::AmendmentNotFound)
}

pub fn set_amendment(env: &Env, amendment: &Amendment) {
    let key = DataKey::Amendment(amendment.id).to_key(env);
    env.storage().persistent().set(&key, amendment);
    env.storage().persistent().extend_ttl(&key, 2592000, 2592000); // ~30 days
}

// Block storage
pub fn get_block(env: &Env, blocker: &Address, blocked: &Address) -> Option<Block> {
    let key = DataKey::BlockRelation(blocker.clone(), blocked.clone()).to_key(env);
    env.storage().persistent().get(&key)
}

pub fn set_block(env: &Env, block: &Block) {
    let key = DataKey::BlockRelation(block.blocker.clone(), block.blocked.clone()).to_key(env);
    env.storage().persistent().set(&key, block);
    env.storage().persistent().extend_ttl(&key, 5184000, 5184000);
}

pub fn remove_block(env: &Env, blocker: &Address, blocked: &Address) {
    let key = DataKey::BlockRelation(blocker.clone(), blocked.clone()).to_key(env);
    env.storage().persistent().remove(&key);
}

// Nonce management for replay protection
pub fn get_user_nonce(env: &Env, user: &Address) -> u64 {
    let key = DataKey::UserNonce(user.clone()).to_key(env);
    env.storage().persistent().get(&key).unwrap_or(0)
}

pub fn increment_user_nonce(env: &Env, user: &Address) -> u64 {
    let current = get_user_nonce(env, user);
    let next = current.checked_add(1).expect("Nonce overflow");
    let key = DataKey::UserNonce(user.clone()).to_key(env);
    env.storage().persistent().set(&key, &next);
    env.storage().persistent().extend_ttl(&key, 5184000, 5184000);
    next
}
