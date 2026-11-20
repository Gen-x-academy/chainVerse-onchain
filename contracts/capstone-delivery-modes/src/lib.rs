#![no_std]

//! Per-call delivery mode configuration for capstone projects.
//!
//! Locations and safety plans stay off-chain. The contract stores only the
//! allowed modes plus commitments to the location and safety documents.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const ALL_MODES_MASK: u32 = 0b11_111;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NoModesSelected = 1,
    UnknownMode = 2,
    UnauthorizedOwner = 3,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Config(BytesN<32>),
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryMode {
    Remote,
    InPerson,
    Hybrid,
    Laboratory,
    Fieldwork,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliveryConfig {
    pub call_id: BytesN<32>,
    pub owner: Address,
    pub allowed_modes: u32,
    pub location_commitment: BytesN<32>,
    pub safety_commitment: BytesN<32>,
    pub updated_at: u64,
}

#[contract]
pub struct CapstoneDeliveryModesContract;

#[contractimpl]
impl CapstoneDeliveryModesContract {
    pub fn set_modes(
        env: Env,
        owner: Address,
        call_id: BytesN<32>,
        allowed_modes: u32,
        location_commitment: BytesN<32>,
        safety_commitment: BytesN<32>,
    ) -> Result<(), ContractError> {
        owner.require_auth();
        if allowed_modes == 0 || allowed_modes & !ALL_MODES_MASK != 0 {
            return Err(ContractError::NoModesSelected);
        }
        let key = DataKey::Config(call_id.clone());
        if let Some(existing) = env.storage().persistent().get::<_, DeliveryConfig>(&key) {
            if existing.owner != owner {
                return Err(ContractError::UnauthorizedOwner);
            }
        }
        let config = DeliveryConfig {
            call_id: call_id.clone(),
            owner: owner.clone(),
            allowed_modes,
            location_commitment,
            safety_commitment,
            updated_at: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&key, &config);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events().publish(
            (symbol_short!("CAPMODE"),),
            (call_id, allowed_modes, owner),
        );
        Ok(())
    }

    pub fn get_config(env: Env, call_id: BytesN<32>) -> Option<DeliveryConfig> {
        env.storage().persistent().get(&DataKey::Config(call_id))
    }

    /// Deny by default: an unconfigured call allows no delivery mode.
    pub fn is_allowed(
        env: Env,
        call_id: BytesN<32>,
        mode: DeliveryMode,
    ) -> Result<bool, ContractError> {
        let config: Option<DeliveryConfig> =
            env.storage().persistent().get(&DataKey::Config(call_id));
        match config {
            Some(config) => Ok(config.allowed_modes & Self::mode_bit(mode) != 0),
            None => Ok(false),
        }
    }

    pub fn mode_bit(mode: DeliveryMode) -> u32 {
        match mode {
            DeliveryMode::Remote => 1 << 0,
            DeliveryMode::InPerson => 1 << 1,
            DeliveryMode::Hybrid => 1 << 2,
            DeliveryMode::Laboratory => 1 << 3,
            DeliveryMode::Fieldwork => 1 << 4,
        }
    }
}
