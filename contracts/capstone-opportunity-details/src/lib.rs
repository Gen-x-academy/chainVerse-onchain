#![no_std]

//! Public detail view for capstone project opportunities.
//!
//! Descriptions, timelines, and terms stay off-chain. The contract stores
//! only commitments to those documents plus public capacity and next-action
//! metadata needed to render a safe detail view.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    AlreadyPublished = 1,
    NotFound = 2,
    UnauthorizedOwner = 3,
    InvalidSeatCount = 4,
    OpportunityClosed = 5,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Detail(BytesN<32>),
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NextAction {
    ViewTerms,
    CheckEligibility,
    SubmitProposal,
    ContactCoordinator,
    None,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpportunityDetail {
    pub opportunity_id: BytesN<32>,
    pub owner: Address,
    pub sponsor: Address,
    pub problem_commitment: BytesN<32>,
    pub outcomes_commitment: BytesN<32>,
    pub prerequisites_commitment: BytesN<32>,
    pub timeline_commitment: BytesN<32>,
    pub roles_commitment: BytesN<32>,
    pub terms_commitment: BytesN<32>,
    pub seats: u32,
    pub next_action: NextAction,
    pub updated_at: u64,
}

#[contract]
pub struct CapstoneOpportunityDetailsContract;

#[contractimpl]
impl CapstoneOpportunityDetailsContract {
    pub fn publish(
        env: Env,
        owner: Address,
        detail: OpportunityDetail,
    ) -> Result<(), ContractError> {
        owner.require_auth();
        if owner != detail.owner || detail.seats == 0 {
            return Err(ContractError::InvalidSeatCount);
        }
        let key = DataKey::Detail(detail.opportunity_id.clone());
        let is_new = !env.storage().persistent().has(&key);
        if !is_new {
            let existing: OpportunityDetail = env
                .storage()
                .persistent()
                .get(&key)
                .ok_or(ContractError::NotFound)?;
            if existing.owner != owner {
                return Err(ContractError::UnauthorizedOwner);
            }
        }
        let mut stored = detail;
        stored.updated_at = env.ledger().timestamp();
        env.storage().persistent().set(&key, &stored);
        Self::extend(&env, &key);
        let event = if is_new {
            symbol_short!("CAPOPNEW")
        } else {
            symbol_short!("CAPOPUPD")
        };
        env.events().publish(
            (event,),
            (stored.opportunity_id, stored.sponsor, stored.seats),
        );
        Ok(())
    }

    pub fn get_detail(env: Env, opportunity_id: BytesN<32>) -> Option<OpportunityDetail> {
        env.storage()
            .persistent()
            .get(&DataKey::Detail(opportunity_id))
    }

    pub fn next_action(
        env: Env,
        opportunity_id: BytesN<32>,
    ) -> Result<NextAction, ContractError> {
        let detail = env
            .storage()
            .persistent()
            .get::<_, OpportunityDetail>(&DataKey::Detail(opportunity_id))
            .ok_or(ContractError::NotFound)?;
        if detail.seats == 0 {
            return Err(ContractError::OpportunityClosed);
        }
        Ok(detail.next_action)
    }

    fn extend(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}
