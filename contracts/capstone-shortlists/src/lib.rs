#![no_std]

//! Bounded, owner-controlled saved shortlists of capstone opportunities.
//!
//! Only commitments to opportunities are stored, never titles, notes, or
//! other private discovery data. Each student controls their own list.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const MAX_ITEMS: u32 = 32;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    ListFull = 1,
    AlreadySaved = 2,
    NotFound = 3,
    InvalidOrder = 4,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Shortlist(Address),
}

#[contract]
pub struct CapstoneShortlistsContract;

#[contractimpl]
impl CapstoneShortlistsContract {
    pub fn save(
        env: Env,
        student: Address,
        opportunity_commitment: BytesN<32>,
    ) -> Result<(), ContractError> {
        student.require_auth();
        let key = DataKey::Shortlist(student.clone());
        let mut list = Self::load(&env, &key);
        if list.len() >= MAX_ITEMS {
            return Err(ContractError::ListFull);
        }
        if Self::index_of(&list, &opportunity_commitment).is_some() {
            return Err(ContractError::AlreadySaved);
        }
        list.push_back(opportunity_commitment.clone());
        Self::store(&env, &key, &list);
        env.events().publish((symbol_short!("CAPSAVE"),), (student, opportunity_commitment));
        Ok(())
    }

    pub fn remove(
        env: Env,
        student: Address,
        opportunity_commitment: BytesN<32>,
    ) -> Result<(), ContractError> {
        student.require_auth();
        let key = DataKey::Shortlist(student.clone());
        let mut list = Self::load(&env, &key);
        let index = Self::index_of(&list, &opportunity_commitment)
            .ok_or(ContractError::NotFound)?;
        list.remove(index);
        Self::store(&env, &key, &list);
        env.events().publish((symbol_short!("CAPUNSV"),), (student, opportunity_commitment));
        Ok(())
    }

    /// Replace the stored order with a permutation of the existing list.
    pub fn reorder(
        env: Env,
        student: Address,
        new_order: Vec<BytesN<32>>,
    ) -> Result<(), ContractError> {
        student.require_auth();
        let key = DataKey::Shortlist(student.clone());
        let list = Self::load(&env, &key);
        if new_order.len() != list.len() {
            return Err(ContractError::InvalidOrder);
        }
        for i in 0..new_order.len() {
            let commitment = new_order.get(i).ok_or(ContractError::InvalidOrder)?;
            if Self::index_of(&list, &commitment).is_none() {
                return Err(ContractError::InvalidOrder);
            }
        }
        Self::store(&env, &key, &new_order);
        env.events().publish((symbol_short!("CAPORD"),), (student,));
        Ok(())
    }

    pub fn get_shortlist(env: Env, student: Address) -> Vec<BytesN<32>> {
        Self::load(&env, &DataKey::Shortlist(student))
    }

    pub fn contains(env: Env, student: Address, opportunity_commitment: BytesN<32>) -> bool {
        let list = Self::load(&env, &DataKey::Shortlist(student));
        Self::index_of(&list, &opportunity_commitment).is_some()
    }

    fn load(env: &Env, key: &DataKey) -> Vec<BytesN<32>> {
        env.storage()
            .persistent()
            .get(key)
            .unwrap_or_else(|| Vec::new(env))
    }

    fn store(env: &Env, key: &DataKey, list: &Vec<BytesN<32>>) {
        env.storage().persistent().set(key, list);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn index_of(list: &Vec<BytesN<32>>, commitment: &BytesN<32>) -> Option<u32> {
        let mut i = 0;
        while i < list.len() {
            if list.get(i).as_ref() == Some(commitment) {
                return Some(i);
            }
            i += 1;
        }
        None
    }
}
