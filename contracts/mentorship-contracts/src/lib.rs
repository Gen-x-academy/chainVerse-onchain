#![no_std]

//! Privacy-minimized, bilateral publication of versioned mentorship terms.
//!
//! Full agreement text and evidence remain off-chain. The contract stores
//! public party addresses, token-denominated amounts, ledger-time dates,
//! content digests, and optional hashes of off-chain evidence references.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const CONTRACT_VERSION: u32 = 2;
const SUPPORTED_SCHEMA_VERSION: u32 = 1;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_VERSIONS_PER_CONTRACT: u32 = 100;
const MAX_PAGE_SIZE: u32 = 50;
const MAX_WAITLIST_SIZE: u32 = 256;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    ContractNotFound = 1,
    VersionNotFound = 2,
    UnsupportedSchemaVersion = 3,
    InvalidTerms = 4,
    InvalidPage = 5,
    InvalidCursor = 6,
    VersionOverflow = 7,
    VersionLimitExceeded = 8,
    PartiesImmutable = 9,
    ProgramNotFound = 10,
    ProgramAlreadyExists = 11,
    NotProgramOrganizer = 12,
    InvalidCapacity = 13,
    CapacityBelowCommitted = 14,
    SeatAlreadyRequested = 15,
    WaitlistFull = 16,
    CapacityFull = 17,
    OfferNotFound = 18,
    OfferExpired = 19,
    OfferNotExpired = 20,
    SeatNotConfirmed = 21,
    WaitlistEmpty = 22,
    WaitlistCorrupt = 23,
    InvalidWaitlistCursor = 24,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    LatestVersion(BytesN<32>),
    ContractVersion(BytesN<32>, u32),
    ProgramCapacity(BytesN<32>),
    Seat(BytesN<32>, Address),
    WaitlistEntry(BytesN<32>, u32),
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SeatState {
    Waitlisted = 1,
    Offered = 2,
    Confirmed = 3,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramCapacity {
    pub organizer: Address,
    pub capacity: u32,
    pub reserved_seats: u32,
    pub confirmed_seats: u32,
    pub waitlist_head: u32,
    pub waitlist_tail: u32,
    pub offer_ttl_seconds: u64,
    pub automatic_promotion: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SeatRecord {
    pub state: SeatState,
    pub requested_at: u64,
    pub offer_expires_at: Option<u64>,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WaitlistPageRequest {
    /// Exclusive sequence cursor; None starts at the current queue head.
    pub after_sequence: Option<u32>,
    pub limit: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WaitlistPage {
    pub participants: Vec<Address>,
    /// Sequence of the final returned participant; None means no next page.
    pub next_cursor: Option<u32>,
}

/// Money is expressed in the asset's smallest units; decimals are resolved
/// from the public asset contract, never inferred from a display string.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Money {
    pub asset: Address,
    pub amount: i128,
}

/// UTC ledger timestamps in seconds. The interval is inclusive at both ends
/// for display; only ordering is enforced by this registry.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DateRange {
    pub starts_at: u64,
    pub ends_at: u64,
}

/// A privacy-safe pointer to off-chain evidence. Neither the content nor its
/// locator is published; only their digests can be recorded.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceReference {
    pub content_hash: BytesN<32>,
    pub reference_hash: BytesN<32>,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContractStatus {
    Published = 1,
}

/// Typed command payload for publishing one immutable agreement version.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishMentorshipCommand {
    pub agreement_id: BytesN<32>,
    pub schema_version: u32,
    pub mentor: Address,
    pub mentee: Address,
    pub fee: Money,
    pub session_count: u32,
    pub period: DateRange,
    pub terms_hash: BytesN<32>,
    pub evidence: Option<EvidenceReference>,
}

/// A published schema snapshot. New publication appends a version and never
/// changes the terms of an earlier version.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MentorshipContractVersion {
    pub agreement_id: BytesN<32>,
    pub schema_version: u32,
    pub version: u32,
    pub mentor: Address,
    pub mentee: Address,
    pub fee: Money,
    pub session_count: u32,
    pub period: DateRange,
    pub terms_hash: BytesN<32>,
    pub evidence: Option<EvidenceReference>,
    pub status: ContractStatus,
    pub published_at: u64,
}

/// Query for a specific version, or the latest published version when None.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GetMentorshipContractQuery {
    pub agreement_id: BytesN<32>,
    pub version: Option<u32>,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageRequest {
    /// Exclusive cursor: zero starts at version one; otherwise continue after
    /// the supplied version number.
    pub after_version: u32,
    pub limit: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListMentorshipContractsQuery {
    pub agreement_id: BytesN<32>,
    pub page: PageRequest,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContractVersionPage {
    pub versions: Vec<MentorshipContractVersion>,
    /// Cursor for the next request; None indicates the final page.
    pub next_cursor: Option<u32>,
}

/// Public event schema emitted once per published version. Private evidence
/// references are intentionally omitted from the event payload.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MentorshipContractPublished {
    pub agreement_id: BytesN<32>,
    pub version: u32,
    pub schema_version: u32,
    pub mentor: Address,
    pub mentee: Address,
    pub terms_hash: BytesN<32>,
    pub status: ContractStatus,
    pub published_at: u64,
}

#[contract]
pub struct MentorshipContractsContract;

#[contractimpl]
impl MentorshipContractsContract {
    /// Both parties authorize every publication, including amendments.
    /// The first version is one; later versions preserve the original parties.
    pub fn publish(
        env: Env,
        command: PublishMentorshipCommand,
    ) -> Result<u32, ContractError> {
        command.mentor.require_auth();
        command.mentee.require_auth();

        if command.schema_version != SUPPORTED_SCHEMA_VERSION {
            return Err(ContractError::UnsupportedSchemaVersion);
        }
        if command.mentor == command.mentee
            || command.fee.amount <= 0
            || command.session_count == 0
            || command.period.starts_at >= command.period.ends_at
        {
            return Err(ContractError::InvalidTerms);
        }

        let latest_key = DataKey::LatestVersion(command.agreement_id.clone());
        let previous_version: u32 = env
            .storage()
            .persistent()
            .get(&latest_key)
            .unwrap_or(0);
        let version = previous_version
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        if version > MAX_VERSIONS_PER_CONTRACT {
            return Err(ContractError::VersionLimitExceeded);
        }

        if previous_version > 0 {
            let previous: MentorshipContractVersion = env
                .storage()
                .persistent()
                .get(&DataKey::ContractVersion(
                    command.agreement_id.clone(),
                    previous_version,
                ))
                .ok_or(ContractError::ContractNotFound)?;
            if previous.mentor != command.mentor || previous.mentee != command.mentee {
                return Err(ContractError::PartiesImmutable);
            }
        }

        let published_at = env.ledger().timestamp();
        let record = MentorshipContractVersion {
            agreement_id: command.agreement_id.clone(),
            schema_version: command.schema_version,
            version,
            mentor: command.mentor.clone(),
            mentee: command.mentee.clone(),
            fee: command.fee,
            session_count: command.session_count,
            period: command.period,
            terms_hash: command.terms_hash.clone(),
            evidence: command.evidence,
            status: ContractStatus::Published,
            published_at,
        };

        let version_key = DataKey::ContractVersion(command.agreement_id.clone(), version);
        env.storage().persistent().set(&version_key, &record);
        env.storage()
            .persistent()
            .extend_ttl(&version_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.storage().persistent().set(&latest_key, &version);
        env.storage()
            .persistent()
            .extend_ttl(&latest_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("MENTPUB"),),
            MentorshipContractPublished {
                agreement_id: command.agreement_id,
                version,
                schema_version: command.schema_version,
                mentor: command.mentor,
                mentee: command.mentee,
                terms_hash: command.terms_hash,
                status: ContractStatus::Published,
                published_at,
            },
        );
        Ok(version)
    }

    pub fn get_contract_version(
        env: Env,
        query: GetMentorshipContractQuery,
    ) -> Result<MentorshipContractVersion, ContractError> {
        let version = match query.version {
            Some(version) => version,
            None => env
                .storage()
                .persistent()
                .get(&DataKey::LatestVersion(query.agreement_id.clone()))
                .ok_or(ContractError::ContractNotFound)?,
        };
        env.storage()
            .persistent()
            .get(&DataKey::ContractVersion(query.agreement_id, version))
            .ok_or(ContractError::VersionNotFound)
    }

    pub fn list_contract_versions(
        env: Env,
        query: ListMentorshipContractsQuery,
    ) -> Result<ContractVersionPage, ContractError> {
        if query.page.limit == 0 || query.page.limit > MAX_PAGE_SIZE {
            return Err(ContractError::InvalidPage);
        }
        let latest: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::LatestVersion(query.agreement_id.clone()))
            .ok_or(ContractError::ContractNotFound)?;
        if query.page.after_version > latest {
            return Err(ContractError::InvalidCursor);
        }

        let mut versions = Vec::new(&env);
        let mut current = query.page.after_version;
        while current < latest && versions.len() < query.page.limit {
            current = current.checked_add(1).ok_or(ContractError::VersionOverflow)?;
            let record: MentorshipContractVersion = env
                .storage()
                .persistent()
                .get(&DataKey::ContractVersion(query.agreement_id.clone(), current))
                .ok_or(ContractError::VersionNotFound)?;
            versions.push_back(record);
        }

        let next_cursor = if current < latest { Some(current) } else { None };
        Ok(ContractVersionPage {
            versions,
            next_cursor,
        })
    }

    pub fn get_latest_version(
        env: Env,
        agreement_id: BytesN<32>,
    ) -> Result<u32, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::LatestVersion(agreement_id))
            .ok_or(ContractError::ContractNotFound)
    }

    /// Create a program's seat inventory. The organizer controls capacity
    /// and promotion policy; participants control their own seat requests.
    pub fn create_program(
        env: Env,
        organizer: Address,
        program_id: BytesN<32>,
        capacity: u32,
        offer_ttl_seconds: u64,
        automatic_promotion: bool,
    ) -> Result<(), ContractError> {
        organizer.require_auth();
        if capacity == 0 || offer_ttl_seconds == 0 {
            return Err(ContractError::InvalidCapacity);
        }
        let key = DataKey::ProgramCapacity(program_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::ProgramAlreadyExists);
        }
        let program = ProgramCapacity {
            organizer,
            capacity,
            reserved_seats: 0,
            confirmed_seats: 0,
            waitlist_head: 0,
            waitlist_tail: 0,
            offer_ttl_seconds,
            automatic_promotion,
        };
        Self::store_program_capacity(&env, &key, &program);
        env.events().publish(
            (symbol_short!("CAPSET"),),
            (program_id, capacity, automatic_promotion),
        );
        Ok(())
    }

    /// Update capacity or promotion policy without invalidating live offers
    /// or confirmed seats.
    pub fn configure_program(
        env: Env,
        organizer: Address,
        program_id: BytesN<32>,
        capacity: u32,
        offer_ttl_seconds: u64,
        automatic_promotion: bool,
    ) -> Result<(), ContractError> {
        let key = DataKey::ProgramCapacity(program_id.clone());
        let mut program = Self::require_organizer(&env, &organizer, &key)?;
        if capacity == 0 || offer_ttl_seconds == 0 {
            return Err(ContractError::InvalidCapacity);
        }
        let committed = program
            .reserved_seats
            .checked_add(program.confirmed_seats)
            .ok_or(ContractError::InvalidCapacity)?;
        if capacity < committed {
            return Err(ContractError::CapacityBelowCommitted);
        }
        program.capacity = capacity;
        program.offer_ttl_seconds = offer_ttl_seconds;
        program.automatic_promotion = automatic_promotion;
        if automatic_promotion {
            while program.waitlist_head < program.waitlist_tail
                && program
                    .reserved_seats
                    .checked_add(program.confirmed_seats)
                    .ok_or(ContractError::InvalidCapacity)?
                    < program.capacity
            {
                Self::promote_next_internal(&env, &program_id, &mut program)?;
            }
        }
        Self::store_program_capacity(&env, &key, &program);
        env.events().publish(
            (symbol_short!("CAPSET"),),
            (program_id, capacity, automatic_promotion),
        );
        Ok(())
    }

    pub fn get_program_capacity(
        env: Env,
        program_id: BytesN<32>,
    ) -> Result<ProgramCapacity, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::ProgramCapacity(program_id))
            .ok_or(ContractError::ProgramNotFound)
    }

    pub fn get_seat_record(
        env: Env,
        program_id: BytesN<32>,
        participant: Address,
    ) -> Option<SeatRecord> {
        env.storage()
            .persistent()
            .get(&DataKey::Seat(program_id, participant))
    }

    /// Read participants in FIFO sequence order using a bounded cursor page.
    pub fn get_waitlist_page(
        env: Env,
        program_id: BytesN<32>,
        page: WaitlistPageRequest,
    ) -> Result<WaitlistPage, ContractError> {
        if page.limit == 0 || page.limit > MAX_PAGE_SIZE {
            return Err(ContractError::InvalidPage);
        }
        let program = Self::load_program_capacity(
            &env,
            &DataKey::ProgramCapacity(program_id.clone()),
        )?;
        let mut sequence = match page.after_sequence {
            Some(cursor) => {
                if cursor >= program.waitlist_tail {
                    return Err(ContractError::InvalidWaitlistCursor);
                }
                cursor
                    .checked_add(1)
                    .ok_or(ContractError::InvalidWaitlistCursor)?
                    .max(program.waitlist_head)
            }
            None => program.waitlist_head,
        };
        let mut participants = Vec::new(&env);
        let mut last_sequence = None;
        while sequence < program.waitlist_tail && participants.len() < page.limit {
            let participant: Address = env
                .storage()
                .persistent()
                .get(&DataKey::WaitlistEntry(program_id.clone(), sequence))
                .ok_or(ContractError::WaitlistCorrupt)?;
            participants.push_back(participant);
            last_sequence = Some(sequence);
            sequence = sequence
                .checked_add(1)
                .ok_or(ContractError::InvalidWaitlistCursor)?;
        }
        let next_cursor = if sequence < program.waitlist_tail {
            last_sequence
        } else {
            None
        };
        Ok(WaitlistPage {
            participants,
            next_cursor,
        })
    }

    /// Request a seat. Available capacity is reserved immediately as an
    /// expiring offer; otherwise the participant is appended to the FIFO.
    pub fn request_seat(
        env: Env,
        participant: Address,
        program_id: BytesN<32>,
    ) -> Result<SeatState, ContractError> {
        participant.require_auth();
        let program_key = DataKey::ProgramCapacity(program_id.clone());
        let mut program = Self::load_program_capacity(&env, &program_key)?;
        let seat_key = DataKey::Seat(program_id.clone(), participant.clone());
        if env.storage().persistent().has(&seat_key) {
            return Err(ContractError::SeatAlreadyRequested);
        }
        let committed = program
            .reserved_seats
            .checked_add(program.confirmed_seats)
            .ok_or(ContractError::InvalidCapacity)?;
        let has_waitlist = program.waitlist_head < program.waitlist_tail;
        let now = env.ledger().timestamp();
        let seat_state = if committed < program.capacity && !has_waitlist {
            let expires_at = now
                .checked_add(program.offer_ttl_seconds)
                .ok_or(ContractError::InvalidCapacity)?;
            program.reserved_seats = program
                .reserved_seats
                .checked_add(1)
                .ok_or(ContractError::InvalidCapacity)?;
            let record = SeatRecord {
                state: SeatState::Offered,
                requested_at: now,
                offer_expires_at: Some(expires_at),
            };
            Self::store_seat_record(&env, &seat_key, &record);
            env.events().publish(
                (symbol_short!("SEATOFR"),),
                (program_id, participant, expires_at),
            );
            SeatState::Offered
        } else {
            let queued = program
                .waitlist_tail
                .checked_sub(program.waitlist_head)
                .ok_or(ContractError::WaitlistCorrupt)?;
            if queued >= MAX_WAITLIST_SIZE {
                return Err(ContractError::WaitlistFull);
            }
            let sequence = program.waitlist_tail;
            program.waitlist_tail = program
                .waitlist_tail
                .checked_add(1)
                .ok_or(ContractError::WaitlistFull)?;
            let waitlist_key = DataKey::WaitlistEntry(program_id.clone(), sequence);
            env.storage()
                .persistent()
                .set(&waitlist_key, &participant);
            env.storage()
                .persistent()
                .extend_ttl(&waitlist_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
            let record = SeatRecord {
                state: SeatState::Waitlisted,
                requested_at: now,
                offer_expires_at: None,
            };
            Self::store_seat_record(&env, &seat_key, &record);
            env.events().publish(
                (symbol_short!("WAITADD"),),
                (program_id, participant, sequence),
            );
            SeatState::Waitlisted
        };
        Self::store_program_capacity(&env, &program_key, &program);
        Ok(seat_state)
    }

    /// Confirm an unexpired offer. Confirmation converts one reservation
    /// into one occupied seat without changing total committed capacity.
    pub fn confirm_seat(
        env: Env,
        participant: Address,
        program_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        participant.require_auth();
        let program_key = DataKey::ProgramCapacity(program_id.clone());
        let mut program = Self::load_program_capacity(&env, &program_key)?;
        let seat_key = DataKey::Seat(program_id.clone(), participant.clone());
        let mut record: SeatRecord = env
            .storage()
            .persistent()
            .get(&seat_key)
            .ok_or(ContractError::OfferNotFound)?;
        if record.state != SeatState::Offered {
            return Err(ContractError::OfferNotFound);
        }
        let expires_at = record.offer_expires_at.ok_or(ContractError::OfferNotFound)?;
        if env.ledger().timestamp() >= expires_at {
            return Err(ContractError::OfferExpired);
        }
        program.reserved_seats = program
            .reserved_seats
            .checked_sub(1)
            .ok_or(ContractError::InvalidCapacity)?;
        program.confirmed_seats = program
            .confirmed_seats
            .checked_add(1)
            .ok_or(ContractError::InvalidCapacity)?;
        record.state = SeatState::Confirmed;
        record.offer_expires_at = None;
        Self::store_seat_record(&env, &seat_key, &record);
        Self::store_program_capacity(&env, &program_key, &program);
        env.events().publish(
            (symbol_short!("SEATCNF"),),
            (program_id, participant),
        );
        Ok(())
    }

    /// Decline an offer. If automatic promotion is enabled, the first
    /// waitlisted participant receives the released reservation.
    pub fn decline_offer(
        env: Env,
        participant: Address,
        program_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        participant.require_auth();
        Self::release_offer(&env, &participant, &program_id, false)
    }

    /// Permissionless expiry settlement. A caller need not be a party; the
    /// ledger timestamp is the sole authority for whether the offer expired.
    pub fn expire_offer(
        env: Env,
        program_id: BytesN<32>,
        participant: Address,
    ) -> Result<(), ContractError> {
        Self::release_offer(&env, &participant, &program_id, true)
    }

    /// A confirmed participant may relinquish their seat.
    pub fn release_confirmed_seat(
        env: Env,
        participant: Address,
        program_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        participant.require_auth();
        let program_key = DataKey::ProgramCapacity(program_id.clone());
        let mut program = Self::load_program_capacity(&env, &program_key)?;
        let seat_key = DataKey::Seat(program_id.clone(), participant.clone());
        let record: SeatRecord = env
            .storage()
            .persistent()
            .get(&seat_key)
            .ok_or(ContractError::SeatNotConfirmed)?;
        if record.state != SeatState::Confirmed {
            return Err(ContractError::SeatNotConfirmed);
        }
        program.confirmed_seats = program
            .confirmed_seats
            .checked_sub(1)
            .ok_or(ContractError::InvalidCapacity)?;
        env.storage().persistent().remove(&seat_key);
        if program.automatic_promotion {
            Self::promote_next_internal(&env, &program_id, &mut program)?;
        }
        Self::store_program_capacity(&env, &program_key, &program);
        env.events().publish(
            (symbol_short!("SEATREL"),),
            (program_id, participant),
        );
        Ok(())
    }

    /// Manually promote the FIFO head when automatic promotion is disabled.
    pub fn promote_next(
        env: Env,
        organizer: Address,
        program_id: BytesN<32>,
    ) -> Result<Address, ContractError> {
        let program_key = DataKey::ProgramCapacity(program_id.clone());
        let mut program = Self::require_organizer(&env, &organizer, &program_key)?;
        let promoted = Self::promote_next_internal(&env, &program_id, &mut program)?
            .ok_or(ContractError::WaitlistEmpty)?;
        Self::store_program_capacity(&env, &program_key, &program);
        Ok(promoted)
    }

    fn release_offer(
        env: &Env,
        participant: &Address,
        program_id: &BytesN<32>,
        require_expired: bool,
    ) -> Result<(), ContractError> {
        let program_key = DataKey::ProgramCapacity(program_id.clone());
        let mut program = Self::load_program_capacity(env, &program_key)?;
        let seat_key = DataKey::Seat(program_id.clone(), participant.clone());
        let record: SeatRecord = env
            .storage()
            .persistent()
            .get(&seat_key)
            .ok_or(ContractError::OfferNotFound)?;
        if record.state != SeatState::Offered {
            return Err(ContractError::OfferNotFound);
        }
        let expires_at = record.offer_expires_at.ok_or(ContractError::OfferNotFound)?;
        let is_expired = env.ledger().timestamp() >= expires_at;
        if require_expired && !is_expired {
            return Err(ContractError::OfferNotExpired);
        }
        if !require_expired && is_expired {
            return Err(ContractError::OfferExpired);
        }
        program.reserved_seats = program
            .reserved_seats
            .checked_sub(1)
            .ok_or(ContractError::InvalidCapacity)?;
        env.storage().persistent().remove(&seat_key);
        if program.automatic_promotion {
            Self::promote_next_internal(env, program_id, &mut program)?;
        }
        Self::store_program_capacity(env, &program_key, &program);
        let event = if require_expired {
            symbol_short!("SEATEXP")
        } else {
            symbol_short!("SEATREL")
        };
        env.events()
            .publish((event,), (program_id.clone(), participant.clone()));
        Ok(())
    }

    fn promote_next_internal(
        env: &Env,
        program_id: &BytesN<32>,
        program: &mut ProgramCapacity,
    ) -> Result<Option<Address>, ContractError> {
        let committed = program
            .reserved_seats
            .checked_add(program.confirmed_seats)
            .ok_or(ContractError::InvalidCapacity)?;
        if committed >= program.capacity {
            return Err(ContractError::CapacityFull);
        }
        if program.waitlist_head >= program.waitlist_tail {
            return Ok(None);
        }
        let sequence = program.waitlist_head;
        let waitlist_key = DataKey::WaitlistEntry(program_id.clone(), sequence);
        let participant: Address = env
            .storage()
            .persistent()
            .get(&waitlist_key)
            .ok_or(ContractError::WaitlistCorrupt)?;
        program.waitlist_head = program
            .waitlist_head
            .checked_add(1)
            .ok_or(ContractError::WaitlistCorrupt)?;
        env.storage().persistent().remove(&waitlist_key);

        let now = env.ledger().timestamp();
        let expires_at = now
            .checked_add(program.offer_ttl_seconds)
            .ok_or(ContractError::InvalidCapacity)?;
        program.reserved_seats = program
            .reserved_seats
            .checked_add(1)
            .ok_or(ContractError::InvalidCapacity)?;
        let seat_key = DataKey::Seat(program_id.clone(), participant.clone());
        let record: SeatRecord = env
            .storage()
            .persistent()
            .get(&seat_key)
            .ok_or(ContractError::WaitlistCorrupt)?;
        if record.state != SeatState::Waitlisted {
            return Err(ContractError::WaitlistCorrupt);
        }
        Self::store_seat_record(
            env,
            &seat_key,
            &SeatRecord {
                state: SeatState::Offered,
                requested_at: record.requested_at,
                offer_expires_at: Some(expires_at),
            },
        );
        env.events().publish(
            (symbol_short!("SEATOFR"),),
            (program_id.clone(), participant.clone(), expires_at),
        );
        Ok(Some(participant))
    }

    fn load_program_capacity(
        env: &Env,
        key: &DataKey,
    ) -> Result<ProgramCapacity, ContractError> {
        env.storage()
            .persistent()
            .get(key)
            .ok_or(ContractError::ProgramNotFound)
    }

    fn require_organizer(
        env: &Env,
        organizer: &Address,
        key: &DataKey,
    ) -> Result<ProgramCapacity, ContractError> {
        let program = Self::load_program_capacity(env, key)?;
        if program.organizer != *organizer {
            return Err(ContractError::NotProgramOrganizer);
        }
        organizer.require_auth();
        Ok(program)
    }

    fn store_program_capacity(env: &Env, key: &DataKey, program: &ProgramCapacity) {
        env.storage().persistent().set(key, program);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn store_seat_record(env: &Env, key: &DataKey, record: &SeatRecord) {
        env.storage().persistent().set(key, record);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;