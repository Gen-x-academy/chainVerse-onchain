#![no_std]

//! Privacy-minimized, append-only audit events for scholarship workflows.
//!
//! Owning contracts call `record` in the same transaction as the operation
//! being audited. The emitted `AUDIT` event is the canonical history; the
//! contract retains only the sequence and chain head needed to link entries.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, xdr::ToXdr, Address,
    BytesN, Env,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    SourceNotRegistered = 4,
    SequenceOverflow = 5,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Source(Address),
    Sequence,
    Head,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditDomain {
    SecurityRead,
    Program,
    Application,
    Review,
    Decision,
    Award,
    Milestone,
    Finance,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditAction {
    Read,
    Create,
    Update,
    Transition,
    Submit,
    Withdraw,
    Review,
    Vote,
    Finalize,
    Appeal,
    Accept,
    Reject,
    Amend,
    Verify,
    Release,
    Deposit,
    Reconcile,
    Refund,
    Recover,
    Configure,
}

/// A single immutable event payload. Hash fields are commitments only; callers
/// must never put personal or otherwise sensitive content directly on-chain.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditRecord {
    pub sequence: u64,
    pub timestamp: u64,
    pub source: Address,
    pub domain: AuditDomain,
    pub action: AuditAction,
    pub entity_hash: BytesN<32>,
    pub actor_hash: BytesN<32>,
    pub payload_hash: BytesN<32>,
    pub previous_hash: BytesN<32>,
    pub record_hash: BytesN<32>,
}

#[contract]
pub struct ScholarshipAuditContract;

#[contractimpl]
impl ScholarshipAuditContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Sequence, &0u64);
        env.storage()
            .instance()
            .set(&DataKey::Head, &BytesN::from_array(&env, &[0; 32]));
        Ok(())
    }

    pub fn register_source(
        env: Env,
        admin: Address,
        source: Address,
    ) -> Result<(), ContractError> {
        require_admin(&env, &admin)?;
        env.storage()
            .persistent()
            .set(&DataKey::Source(source.clone()), &true);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Source(source), RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }

    pub fn revoke_source(
        env: Env,
        admin: Address,
        source: Address,
    ) -> Result<(), ContractError> {
        require_admin(&env, &admin)?;
        env.storage()
            .persistent()
            .set(&DataKey::Source(source.clone()), &false);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Source(source), RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }

    pub fn record(
        env: Env,
        source: Address,
        domain: AuditDomain,
        action: AuditAction,
        entity_hash: BytesN<32>,
        actor_hash: BytesN<32>,
        payload_hash: BytesN<32>,
    ) -> Result<AuditRecord, ContractError> {
        let source_key = DataKey::Source(source.clone());
        if !env
            .storage()
            .persistent()
            .get::<_, bool>(&source_key)
            .unwrap_or(false)
        {
            return Err(ContractError::SourceNotRegistered);
        }
        source.require_auth();

        let sequence = env
            .storage()
            .instance()
            .get::<_, u64>(&DataKey::Sequence)
            .ok_or(ContractError::NotInitialized)?
            .checked_add(1)
            .ok_or(ContractError::SequenceOverflow)?;
        let previous_hash = env
            .storage()
            .instance()
            .get::<_, BytesN<32>>(&DataKey::Head)
            .ok_or(ContractError::NotInitialized)?;
        let timestamp = env.ledger().timestamp();
        let hash_input = (
            sequence,
            timestamp,
            source.clone(),
            domain,
            action,
            entity_hash.clone(),
            actor_hash.clone(),
            payload_hash.clone(),
            previous_hash.clone(),
        )
            .to_xdr(&env);
        let record_hash = env.crypto().sha256(&hash_input);
        let record = AuditRecord {
            sequence,
            timestamp,
            source,
            domain,
            action,
            entity_hash,
            actor_hash,
            payload_hash,
            previous_hash,
            record_hash: record_hash.clone(),
        };

        env.storage()
            .instance()
            .set(&DataKey::Sequence, &sequence);
        env.storage()
            .instance()
            .set(&DataKey::Head, &record_hash);
        env.storage()
            .instance()
            .extend_ttl(RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.storage()
            .persistent()
            .extend_ttl(&source_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events()
            .publish((symbol_short!("AUDIT"), sequence), record.clone());
        Ok(record)
    }

    pub fn get_head(env: Env) -> Result<(u64, BytesN<32>), ContractError> {
        let sequence = env
            .storage()
            .instance()
            .get(&DataKey::Sequence)
            .ok_or(ContractError::NotInitialized)?;
        let head = env
            .storage()
            .instance()
            .get(&DataKey::Head)
            .ok_or(ContractError::NotInitialized)?;
        Ok((sequence, head))
    }
}

fn require_admin(env: &Env, admin: &Address) -> Result<(), ContractError> {
    let configured = env
        .storage()
        .instance()
        .get::<_, Address>(&DataKey::Admin)
        .ok_or(ContractError::NotInitialized)?;
    if configured != *admin {
        return Err(ContractError::NotAdmin);
    }
    admin.require_auth();
    Ok(())
}

#[cfg(test)]
mod tests;