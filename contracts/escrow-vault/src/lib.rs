#![no_std]

const VAULT_MIN_TTL: u32 = 100_000;
const VAULT_MAX_TTL: u32 = 500_000;

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, BytesN, Env, Vec};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum VaultError {
    NotFound = 1,
    NotPending = 2,
    Unauthorized = 3,
    AlreadyVoted = 4,
    EmptyApprovers = 5,
    DuplicateApprover = 6,
    InvalidAmount = 7,
}

#[contracttype]
#[derive(Clone)]
pub enum VaultStatus { Pending, Released, Cancelled }

#[contracttype]
#[derive(Clone)]
pub struct Vault {
    pub depositor: Address,
    pub recipient: Address,
    pub token: Address,
    pub amount: i128,
    pub approvers: Vec<Address>,
    pub approvals: u32,
    pub threshold: u32,
    pub status: VaultStatus,
}

#[contracttype]
pub enum DataKey { Vault(BytesN<32>) }

#[contract]
pub struct EscrowVault;

#[contractimpl]
impl EscrowVault {
    pub fn create_vault(
        env: Env,
        depositor: Address,
        recipient: Address,
        token: Address,
        amount: i128,
        approvers: Vec<Address>,
        threshold: u32,
    ) -> Result<BytesN<32>, VaultError> {
        if approvers.is_empty() {
            return Err(VaultError::EmptyApprovers);
        }
        if amount <= 0 {
            return Err(VaultError::InvalidAmount);
        }
        depositor.require_auth();
        soroban_sdk::token::Client::new(&env, &token)
            .transfer(&depositor, &env.current_contract_address(), &amount);
        let id: BytesN<32> = env.crypto().sha256(
            &soroban_sdk::Bytes::from_slice(&env, &env.ledger().timestamp().to_be_bytes())
        ).into();
        let vault = Vault {
            depositor, recipient, token, amount, approvers, approvals: 0, threshold,
            status: VaultStatus::Pending,
        };
        env.storage().persistent().set(&DataKey::Vault(id.clone()), &vault);
        env.storage().persistent().extend_ttl(&DataKey::Vault(id.clone()), VAULT_MIN_TTL, VAULT_MAX_TTL);
        Ok(id)
    }
}
