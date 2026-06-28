#![no_std]

const VAULT_MIN_TTL: u32 = 100_000;
const VAULT_MAX_TTL: u32 = 500_000;

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec};

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
pub enum DataKey { Vault(BytesN<32>), VaultCount }

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
        for i in 0..approvers.len() {
            for j in (i + 1)..approvers.len() {
                if approvers.get(i) == approvers.get(j) {
                    return Err(VaultError::DuplicateApprover);
                }
            }
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
            depositor: depositor.clone(),
            recipient,
            token,
            amount,
            approvers,
            approvals: 0,
            threshold,
            status: VaultStatus::Pending,
        };
        env.storage().persistent().set(&DataKey::Vault(id.clone()), &vault);
        env.storage().persistent().extend_ttl(&DataKey::Vault(id.clone()), VAULT_MIN_TTL, VAULT_MAX_TTL);
        // #639 — emit event for audit trail
        env.events().publish((symbol_short!("VAULT_NEW"),), (id.clone(), depositor.clone(), amount));
        Ok(id)
    }

    pub fn approve(
        env: Env,
        caller: Address,
        id: BytesN<32>,
    ) -> Result<(), VaultError> {
        caller.require_auth();
        let mut vault: Vault = env.storage().persistent()
            .get(&DataKey::Vault(id.clone()))
            .ok_or(VaultError::NotFound)?;
        match vault.status {
            VaultStatus::Pending => {}
            _ => return Err(VaultError::NotPending),
        }
        // #640 — prevent depositor from self-approving their own vault release
        if caller == vault.depositor && vault.approvers.contains(&caller) {
            return Err(VaultError::Unauthorized);
        }
        if !vault.approvers.contains(&caller) {
            return Err(VaultError::Unauthorized);
        }
        vault.approvals += 1;
        if vault.approvals >= vault.threshold {
            vault.status = VaultStatus::Released;
            soroban_sdk::token::Client::new(&env, &vault.token)
                .transfer(&env.current_contract_address(), &vault.recipient, &vault.amount);
        }
        env.storage().persistent().set(&DataKey::Vault(id.clone()), &vault);
        env.storage().persistent().extend_ttl(&DataKey::Vault(id.clone()), VAULT_MIN_TTL, VAULT_MAX_TTL);
        Ok(())
    }
}
