#![no_std]

//! Privacy-minimized mentorship sponsorship ledger.
//!
//! An organization or guardian funds eligible engagements under an explicit
//! budget and per-engagement limit. Only public identifiers, amounts, and
//! commitments to off-chain engagement details are stored on-chain.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    Unauthorized = 1,
    SponsorshipAlreadyExists = 2,
    SponsorshipNotFound = 3,
    InvalidAmount = 4,
    InvalidBudget = 5,
    InvalidExpiry = 6,
    SponsorshipClosed = 7,
    SponsorshipExpired = 8,
    BudgetExceeded = 9,
    ReservationAlreadyExists = 10,
    ReservationNotFound = 11,
    ReservationNotActive = 12,
    SettlementExceedsReservation = 13,
    OutstandingReservations = 14,
    ArithmeticOverflow = 15,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Sponsorship(BytesN<32>),
    Reservation(BytesN<32>, BytesN<32>),
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SponsorshipStatus {
    Active,
    Closed,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReservationStatus {
    Reserved,
    Released,
    Settled,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sponsorship {
    pub id: BytesN<32>,
    pub sponsor: Address,
    pub program_id: BytesN<32>,
    pub budget: i128,
    pub reserved: i128,
    pub spent: i128,
    pub max_per_engagement: i128,
    pub expires_at: u64,
    pub status: SponsorshipStatus,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SponsorshipReservation {
    pub id: BytesN<32>,
    pub engagement_ref: BytesN<32>,
    pub amount: i128,
    pub status: ReservationStatus,
}

#[contract]
pub struct MentorshipSponsorshipContract;

#[contractimpl]
impl MentorshipSponsorshipContract {
    pub fn create_sponsorship(
        env: Env,
        sponsor: Address,
        sponsorship_id: BytesN<32>,
        program_id: BytesN<32>,
        budget: i128,
        max_per_engagement: i128,
        expires_at: u64,
    ) -> Result<(), ContractError> {
        sponsor.require_auth();

        let key = DataKey::Sponsorship(sponsorship_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::SponsorshipAlreadyExists);
        }
        if budget <= 0 || max_per_engagement <= 0 || max_per_engagement > budget {
            return Err(ContractError::InvalidBudget);
        }
        if expires_at <= env.ledger().timestamp() {
            return Err(ContractError::InvalidExpiry);
        }

        let sponsorship = Sponsorship {
            id: sponsorship_id.clone(),
            sponsor: sponsor.clone(),
            program_id,
            budget,
            reserved: 0,
            spent: 0,
            max_per_engagement,
            expires_at,
            status: SponsorshipStatus::Active,
        };
        Self::write_sponsorship(&env, &key, &sponsorship);
        env.events()
            .publish((symbol_short!("SPONNEW"),), (sponsorship_id, sponsor));
        Ok(())
    }

    /// Reserve funds for an engagement. Reservation ids are unique, so a retry
    /// of the same request is a no-op and cannot duplicate the hold.
    pub fn reserve(
        env: Env,
        sponsor: Address,
        sponsorship_id: BytesN<32>,
        reservation_id: BytesN<32>,
        engagement_ref: BytesN<32>,
        amount: i128,
    ) -> Result<(), ContractError> {
        sponsor.require_auth();

        let key = DataKey::Sponsorship(sponsorship_id.clone());
        let mut sponsorship: Sponsorship = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::SponsorshipNotFound)?;
        Self::require_sponsor(&sponsorship, &sponsor)?;
        Self::require_active(&env, &sponsorship)?;

        let reservation_key =
            DataKey::Reservation(sponsorship_id.clone(), reservation_id.clone());
        if let Some(existing) = env
            .storage()
            .persistent()
            .get::<_, SponsorshipReservation>(&reservation_key)
        {
            if existing.engagement_ref == engagement_ref && existing.amount == amount {
                return Ok(());
            }
            return Err(ContractError::ReservationAlreadyExists);
        }
        if amount <= 0 || amount > sponsorship.max_per_engagement {
            return Err(ContractError::InvalidAmount);
        }
        let reserved = sponsorship
            .reserved
            .checked_add(amount)
            .ok_or(ContractError::ArithmeticOverflow)?;
        if reserved > sponsorship.budget {
            return Err(ContractError::BudgetExceeded);
        }

        let reservation = SponsorshipReservation {
            id: reservation_id.clone(),
            engagement_ref,
            amount,
            status: ReservationStatus::Reserved,
        };
        sponsorship.reserved = reserved;
        Self::write_sponsorship(&env, &key, &sponsorship);
        Self::write_reservation(&env, &reservation_key, &reservation);
        env.events().publish(
            (symbol_short!("SPONRES"),),
            (sponsorship_id, reservation_id, amount),
        );
        Ok(())
    }

    /// Release an unused reservation. Releasing twice is a no-op.
    pub fn release_reservation(
        env: Env,
        sponsor: Address,
        sponsorship_id: BytesN<32>,
        reservation_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        sponsor.require_auth();

        let key = DataKey::Sponsorship(sponsorship_id.clone());
        let mut sponsorship: Sponsorship = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::SponsorshipNotFound)?;
        Self::require_sponsor(&sponsorship, &sponsor)?;

        let reservation_key =
            DataKey::Reservation(sponsorship_id.clone(), reservation_id.clone());
        let mut reservation: SponsorshipReservation = env
            .storage()
            .persistent()
            .get(&reservation_key)
            .ok_or(ContractError::ReservationNotFound)?;
        if reservation.status == ReservationStatus::Released {
            return Ok(());
        }
        if reservation.status != ReservationStatus::Reserved {
            return Err(ContractError::ReservationNotActive);
        }

        sponsorship.reserved = sponsorship
            .reserved
            .checked_sub(reservation.amount)
            .ok_or(ContractError::ArithmeticOverflow)?;
        reservation.status = ReservationStatus::Released;
        Self::write_sponsorship(&env, &key, &sponsorship);
        Self::write_reservation(&env, &reservation_key, &reservation);
        env.events().publish(
            (symbol_short!("SPONREL"),),
            (sponsorship_id, reservation_id),
        );
        Ok(())
    }

    /// Settle a reservation against an attested spend amount. Settling twice
    /// is a no-op, and the settled amount can never exceed the hold.
    pub fn settle(
        env: Env,
        sponsor: Address,
        sponsorship_id: BytesN<32>,
        reservation_id: BytesN<32>,
        amount: i128,
    ) -> Result<(), ContractError> {
        sponsor.require_auth();

        let key = DataKey::Sponsorship(sponsorship_id.clone());
        let mut sponsorship: Sponsorship = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::SponsorshipNotFound)?;
        Self::require_sponsor(&sponsorship, &sponsor)?;
        Self::require_active(&env, &sponsorship)?;

        let reservation_key =
            DataKey::Reservation(sponsorship_id.clone(), reservation_id.clone());
        let mut reservation: SponsorshipReservation = env
            .storage()
            .persistent()
            .get(&reservation_key)
            .ok_or(ContractError::ReservationNotFound)?;
        if reservation.status == ReservationStatus::Settled {
            return Ok(());
        }
        if reservation.status != ReservationStatus::Reserved {
            return Err(ContractError::ReservationNotActive);
        }
        if amount <= 0 || amount > reservation.amount {
            return Err(ContractError::SettlementExceedsReservation);
        }

        sponsorship.reserved = sponsorship
            .reserved
            .checked_sub(reservation.amount)
            .ok_or(ContractError::ArithmeticOverflow)?;
        sponsorship.spent = sponsorship
            .spent
            .checked_add(amount)
            .ok_or(ContractError::ArithmeticOverflow)?;
        reservation.amount = amount;
        reservation.status = ReservationStatus::Settled;
        Self::write_sponsorship(&env, &key, &sponsorship);
        Self::write_reservation(&env, &reservation_key, &reservation);
        env.events().publish(
            (symbol_short!("SPONSET"),),
            (sponsorship_id, reservation_id, amount),
        );
        Ok(())
    }

    pub fn close_sponsorship(
        env: Env,
        sponsor: Address,
        sponsorship_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        sponsor.require_auth();

        let key = DataKey::Sponsorship(sponsorship_id.clone());
        let mut sponsorship: Sponsorship = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::SponsorshipNotFound)?;
        Self::require_sponsor(&sponsorship, &sponsor)?;
        if sponsorship.status == SponsorshipStatus::Closed {
            return Ok(());
        }
        if sponsorship.reserved != 0 {
            return Err(ContractError::OutstandingReservations);
        }
        sponsorship.status = SponsorshipStatus::Closed;
        Self::write_sponsorship(&env, &key, &sponsorship);
        env.events()
            .publish((symbol_short!("SPONCLO"),), (sponsorship_id,));
        Ok(())
    }

    pub fn get_sponsorship(env: Env, sponsorship_id: BytesN<32>) -> Option<Sponsorship> {
        env.storage()
            .persistent()
            .get(&DataKey::Sponsorship(sponsorship_id))
    }

    pub fn get_reservation(
        env: Env,
        sponsorship_id: BytesN<32>,
        reservation_id: BytesN<32>,
    ) -> Option<SponsorshipReservation> {
        env.storage()
            .persistent()
            .get(&DataKey::Reservation(sponsorship_id, reservation_id))
    }

    fn require_sponsor(
        sponsorship: &Sponsorship,
        sponsor: &Address,
    ) -> Result<(), ContractError> {
        if &sponsorship.sponsor != sponsor {
            return Err(ContractError::Unauthorized);
        }
        Ok(())
    }

    fn require_active(env: &Env, sponsorship: &Sponsorship) -> Result<(), ContractError> {
        if sponsorship.status != SponsorshipStatus::Active {
            return Err(ContractError::SponsorshipClosed);
        }
        if env.ledger().timestamp() > sponsorship.expires_at {
            return Err(ContractError::SponsorshipExpired);
        }
        Ok(())
    }

    fn write_sponsorship(env: &Env, key: &DataKey, sponsorship: &Sponsorship) {
        env.storage().persistent().set(key, sponsorship);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn write_reservation(env: &Env, key: &DataKey, reservation: &SponsorshipReservation) {
        env.storage().persistent().set(key, reservation);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}
