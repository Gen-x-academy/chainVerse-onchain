#![no_std]

//! Participant-controlled visibility rules for capstone profile fields.
//!
//! Only the boolean visibility decisions are stored on-chain. Profile content
//! itself remains off-chain, and an audience with no explicit rule sees nothing.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NothingToUpdate = 1,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Visibility(Address, Audience),
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Audience {
    Public,
    Participants,
    Supervisors,
    Reviewers,
    Sponsors,
    Administrators,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileField {
    Portfolio,
    Skills,
    Affiliation,
    Availability,
    Contact,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VisibilitySettings {
    pub portfolio: bool,
    pub skills: bool,
    pub affiliation: bool,
    pub availability: bool,
    pub contact: bool,
}

#[contract]
pub struct CapstoneProfileVisibilityContract;

#[contractimpl]
impl CapstoneProfileVisibilityContract {
    pub fn set_visibility(
        env: Env,
        participant: Address,
        audience: Audience,
        settings: VisibilitySettings,
    ) -> Result<(), ContractError> {
        participant.require_auth();
        if !settings.portfolio
            && !settings.skills
            && !settings.affiliation
            && !settings.availability
            && !settings.contact
        {
            return Err(ContractError::NothingToUpdate);
        }
        let key = DataKey::Visibility(participant.clone(), audience);
        env.storage().persistent().set(&key, &settings);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events().publish(
            (symbol_short!("CAPVIS"),),
            (participant, audience, settings),
        );
        Ok(())
    }

    pub fn get_visibility(
        env: Env,
        participant: Address,
        audience: Audience,
    ) -> Option<VisibilitySettings> {
        env.storage()
            .persistent()
            .get(&DataKey::Visibility(participant, audience))
    }

    /// Deny by default: a field is visible only when an explicit rule allows it.
    pub fn is_visible(
        env: Env,
        participant: Address,
        audience: Audience,
        field: ProfileField,
    ) -> bool {
        let settings: Option<VisibilitySettings> = env
            .storage()
            .persistent()
            .get(&DataKey::Visibility(participant, audience));
        match (settings, field) {
            (Some(settings), ProfileField::Portfolio) => settings.portfolio,
            (Some(settings), ProfileField::Skills) => settings.skills,
            (Some(settings), ProfileField::Affiliation) => settings.affiliation,
            (Some(settings), ProfileField::Availability) => settings.availability,
            (Some(settings), ProfileField::Contact) => settings.contact,
            (None, _) => false,
        }
    }
}
