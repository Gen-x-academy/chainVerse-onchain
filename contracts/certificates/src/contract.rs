use soroban_sdk::{contract, contractimpl, Address, Env};

use crate::storage::*;
use crate::errors::ContractError;
use crate::events::*;

#[contract]
pub struct CertificateContract;

#[contractimpl]
impl CertificateContract {

    // Mint certificate (allowed)
    pub fn mint(e: Env, to: Address, cert_id: u64) -> Result<(), ContractError> {
        to.require_auth();

        if get_certificate_owner(&e, cert_id).is_some() {
            return Err(ContractError::CertificateExists);
        }

        set_certificate_owner(&e, cert_id, &to);

        certificate_minted(&e, to, cert_id);

        Ok(())
    }

    // 🔒 Soulbound Enforcement
    pub fn transfer(
        _e: Env,
        _from: Address,
        _to: Address,
        _cert_id: u64,
    ) -> Result<(), ContractError> {
        // Explicit rejection
        Err(ContractError::SoulboundTransferNotAllowed)
    }

    // View owner
    pub fn owner_of(e: Env, cert_id: u64) -> Option<Address> {
        get_certificate_owner(&e, cert_id)
    }
}