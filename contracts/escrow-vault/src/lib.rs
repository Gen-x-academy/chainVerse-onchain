#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, contracterror, Address, Env, Vec};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum VaultError {
    NotFound      = 1,
    NotPending    = 2,
    Unauthorized  = 3,
    AlreadyVoted  = 4,
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, PartialEq)]
pub enum VaultStatus {
    Pending,
    Released,
    Cancelled,
}

#[contracttype]
#[derive(Clone)]
pub struct Vault {
    pub depositor:  Address,
    pub recipient:  Address,
    pub token:      Address,
    pub amount:     i128,
    pub approvers:  Vec<Address>,
    pub approvals:  Vec<Address>,
    pub status:     VaultStatus,
}

#[contracttype]
pub enum DataKey {
    Vault(u64),
    NextId,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct EscrowVault;

#[contractimpl]
impl EscrowVault {
    /// Create a new vault. The depositor provides the approver set.
    pub fn create_vault(
        env: Env,
        depositor: Address,
        recipient: Address,
        token: Address,
        amount: i128,
        approvers: Vec<Address>,
    ) -> u64 {
        depositor.require_auth();
        let id = Self::next_id(&env);
        let vault = Vault {
            depositor,
            recipient,
            token,
            amount,
            approvers,
            approvals: Vec::new(&env),
            status: VaultStatus::Pending,
        };
        env.storage().instance().set(&DataKey::Vault(id), &vault);
        id
    }

    /// Approve release of a vault. Only addresses in the approver set may call this.
    /// Funds are released once all approvers have approved.
    pub fn approve_release(env: Env, caller: Address, vault_id: u64) -> Result<(), VaultError> {
        caller.require_auth();

        let mut vault: Vault = env
            .storage()
            .instance()
            .get(&DataKey::Vault(vault_id))
            .ok_or(VaultError::NotFound)?;

        if vault.status != VaultStatus::Pending {
            return Err(VaultError::NotPending);
        }

        // Caller must be in the approver set
        if !vault.approvers.contains(&caller) {
            return Err(VaultError::Unauthorized);
        }

        // Prevent double-voting
        if vault.approvals.contains(&caller) {
            return Err(VaultError::AlreadyVoted);
        }

        vault.approvals.push_back(caller);

        // Release when all approvers have signed off
        if vault.approvals.len() == vault.approvers.len() {
            vault.status = VaultStatus::Released;
        }

        env.storage().instance().set(&DataKey::Vault(vault_id), &vault);
        Ok(())
    }

    /// Returns the vault record for the given id.
    pub fn get_vault(env: Env, vault_id: u64) -> Result<Vault, VaultError> {
        env.storage()
            .instance()
            .get(&DataKey::Vault(vault_id))
            .ok_or(VaultError::NotFound)
    }

    // -----------------------------------------------------------------------
    // Internal
    // -----------------------------------------------------------------------

    fn next_id(env: &Env) -> u64 {
        let id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextId)
            .unwrap_or(0u64);
        env.storage().instance().set(&DataKey::NextId, &(id + 1));
        id
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::Address as _, vec, Env};

    fn setup() -> (Env, EscrowVaultClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register_contract(None, EscrowVault);
        let client = EscrowVaultClient::new(&env, &id);
        (env, client)
    }

    #[test]
    fn test_approve_release_on_already_released_escrow_fails() {
        let (env, client) = setup();

        let depositor = Address::generate(&env);
        let recipient = Address::generate(&env);
        let token     = Address::generate(&env);
        let approver  = Address::generate(&env);

        let vault_id = client.create_vault(
            &depositor,
            &recipient,
            &token,
            &100,
            &vec![&env, approver.clone()],
        );

        // First approval should succeed and release the vault
        client.approve_release(&approver, &vault_id);

        // Second approval attempt should fail with NotPending
        let result = client.try_approve_release(&approver, &vault_id);
        assert!(result.is_err(), "approve_release on already released escrow must fail");
        if let Err(err) = result {
            assert_eq!(err, VaultError::NotPending);
        }
    }

    // Issue #98 — approve_release from unauthorised caller must be rejected
    #[test]
    fn test_approve_release_from_unauthorised_caller_is_rejected() {
        let (env, client) = setup();

        let depositor  = Address::generate(&env);
        let recipient  = Address::generate(&env);
        let token      = Address::generate(&env);
        let approver   = Address::generate(&env);
        let outsider   = Address::generate(&env); // NOT in approver set

        let vault_id = client.create_vault(
            &depositor,
            &recipient,
            &token,
            &1000,
            &vec![&env, approver.clone()],
        );

        // Outsider attempts to approve — must fail with Unauthorized
        let result = client.try_approve_release(&outsider, &vault_id);
        assert!(result.is_err(), "approve_release from non-approver must be rejected");

        // Vault must still be Pending — no fund movement
        let vault = client.get_vault(&vault_id);
        assert_eq!(vault.status, VaultStatus::Pending);
        assert_eq!(vault.approvals.len(), 0);
    }

    #[test]
    fn test_approve_release_from_authorised_caller_succeeds() {
        let (env, client) = setup();

        let depositor = Address::generate(&env);
        let recipient = Address::generate(&env);
        let token     = Address::generate(&env);
        let approver  = Address::generate(&env);

        let vault_id = client.create_vault(
            &depositor,
            &recipient,
            &token,
            &500,
            &vec![&env, approver.clone()],
        );

        client.approve_release(&approver, &vault_id);

        let vault = client.get_vault(&vault_id);
        assert_eq!(vault.status, VaultStatus::Released);
    }

    #[test]
    fn test_approve_release_requires_all_approvers() {
        let (env, client) = setup();

        let depositor = Address::generate(&env);
        let recipient = Address::generate(&env);
        let token     = Address::generate(&env);
        let a1        = Address::generate(&env);
        let a2        = Address::generate(&env);

        let vault_id = client.create_vault(
            &depositor,
            &recipient,
            &token,
            &200,
            &vec![&env, a1.clone(), a2.clone()],
        );

        // Only first approver — still pending
        client.approve_release(&a1, &vault_id);
        let vault = client.get_vault(&vault_id);
        assert_eq!(vault.status, VaultStatus::Pending);

        // Second approver — now released
        client.approve_release(&a2, &vault_id);
        let vault = client.get_vault(&vault_id);
        assert_eq!(vault.status, VaultStatus::Released);
    }
}
