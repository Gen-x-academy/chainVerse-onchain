# Mentor Profiles Contract

## Overview

The `mentor-profiles` contract is a Soroban smart contract that enables mentors to publish verified profiles on the Stellar blockchain. It supports:

- **Profile Publication**: Mentors publish expertise, availability, rates, and policies with hash-based privacy
- **Availability Management**: Update timezone, delivery modes, and availability status
- **Rating System**: Backend-authorized rating submission with reputation scoring
- **Archive/Restore**: Mentors can archive profiles and restore them later
- **Admin Functions**: Two-step admin transfer and backend signer management

## Key Features

### Privacy-First Design

All sensitive data (full names, bios, policy documents, review content) is stored off-chain. On-chain, we commit only:
- 32-byte hashes of expertise, policies, and reviews
- Status flags and enumerations
- Numeric ratings and derived reputation scores
- Addresses and ledger timestamps

### Profile Data Structure

```rust
pub struct MentorProfile {
    pub mentor_id: Address,                    // Mentor's Soroban address
    pub expertise_hash: Bytes,                 // Hash of expertise metadata
    pub availability_status: AvailabilityStatus,
    pub timezone_offset: i32,                  // UTC offset (-12 to +14)
    pub delivery_modes: u32,                   // Bitmask of supported modes
    pub rate_per_hour: u128,                   // In stroops (Stellar microunits)
    pub rating_count: u32,                     // Total ratings received
    pub rating_sum: u32,                       // Sum of all rating scores
    pub reputation_score: u32,                 // Derived score (0-100)
    pub public_attestation_hash: Bytes,        // Hash of endorsements
    pub policy_hash: Bytes,                    // Hash of engagement policies
    pub created_at: u32,                       // Ledger sequence
    pub updated_at: u32,                       // Ledger sequence
}
```

### Contract Functions

#### Initialization

- **`init(admin, backend_signer)`** - Initialize with admin and authorized backend rating submitter
  - Requires admin authorization
  - Prevents double initialization

#### Profile Management

- **`publish_profile(expertise_hash, timezone_offset, delivery_modes, rate_per_hour, public_attestation_hash, policy_hash)`**
  - Mentor publishes a new profile
  - Validates all inputs (hash lengths, timezone range, rate > 0)
  - Prevents duplicate profiles
  - Emits `profile_published` event

- **`get_profile(mentor_id)`** - Public read of mentor profile
  - Returns full profile or error if not found

- **`update_availability(status, timezone_offset)`**
  - Mentor updates availability status and timezone
  - Status: 0=Available, 1=Booked, 2=Archived

- **`archive_profile()` / `restore_profile()`**
  - Mentor archives profile (becomes invisible to directory)
  - Mentor restores archived profile to Available

#### Rating System

- **`record_rating(mentor_id, score, review_hash, engagement_hash)`**
  - Backend signer only
  - Score must be 1-5
  - Updates rating_count, rating_sum, and reputation_score
  - Reputation formula: `min(avg_rating * multiplier, 100)`
  - Default multiplier: 10x (e.g., avg rating of 4 → reputation 40)
  - Emits `rating_updated` event

#### Admin Functions

- **`propose_admin_transfer(new_admin)`** - Admin proposes successor
- **`accept_admin_transfer()`** - Proposed admin accepts transfer (two-step)
- **`set_backend_signer(new_signer)`** - Admin updates backend rating signer
- **`get_admin()` / `get_backend_signer()`** - Public reads

## Events

All events follow the pattern: `("mentor", "<event_name>"), (data...)`

1. **`profile_published`**: `(mentor_id, expertise_hash, rate_per_hour)`
2. **`availability_updated`**: `(mentor_id, status)`
3. **`rating_updated`**: `(mentor_id, avg_rating, rating_count)`
4. **`profile_archived`**: `(mentor_id,)`
5. **`profile_restored`**: `(mentor_id,)`
6. **`reputation_updated`**: `(mentor_id, new_score)`

## Authorization

- **Profile Operations**: Only the mentor (caller) can publish, update, archive, or restore their own profile
- **Rating Submissions**: Only the backend signer address can call `record_rating`
- **Admin Operations**: Only current admin can propose transfer or set backend signer
- **Public Reads**: Anyone can call `get_profile` to view published profiles

## Error Codes

| Code | Error | Meaning |
|------|-------|---------|
| 1 | UnauthorizedAdmin | Caller is not the admin |
| 2 | OnlyMentorAllowed | Only the mentor can perform this operation |
| 3 | ProfileNotFound | No profile exists for the given mentor |
| 4 | InvalidStatus | Invalid availability status (not 0, 1, or 2) |
| 5 | InvalidTimezone | Timezone offset outside -12 to +14 range |
| 6 | InvalidDeliveryMode | Delivery mode bitmask is 0 or invalid |
| 7 | InvalidRate | Rate per hour is 0 |
| 8 | InvalidRatingScore | Rating score not in 1-5 range |
| 9 | InvalidTimeWindow | Invalid time window provided |
| 10 | ProfileAlreadyExists | Mentor already has a published profile |
| 11 | InvalidHashLength | Hash is not exactly 32 bytes |
| 12 | AuthenticationFailed | Authentication/authorization failed |
| 13 | InvalidInput | Generic invalid input |
| 14 | UnauthorizedBackend | Caller is not the authorized backend signer |
| 15 | ReputationCalcError | Reputation calculation overflow |
| 16 | StorageFailed | Storage operation failed |

## Testing

The contract includes comprehensive tests covering:

- **Initialization** (`init_tests.rs`)
  - Admin and backend signer setup
  - Double initialization prevention
  - Authorization requirements

- **Profile Operations** (`profile_tests.rs`)
  - Profile publication with validation
  - Hash length validation
  - Timezone range validation
  - Rate validation
  - Duplicate profile prevention
  - Availability updates
  - Archive/restore workflow
  - Non-existent profile errors

- **Rating System** (`rating_tests.rs`)
  - Single and multiple ratings
  - Invalid score rejection
  - Unauthorized backend rejection
  - Hash validation
  - Reputation score calculation
  - Non-existent profile handling

Run tests with:
```bash
cd contracts/mentor-profiles
cargo test --lib
cargo test --test '*'
```

## Integration with Other Contracts

### Mentor Directory (`mentor-directory`)
- Directory indexes published profiles for searchable discovery
- Pulls profile data (rate, timezone, delivery modes) for directory entries

### Mentor Programs (`mentor-programs`)
- Programs reference mentor profiles for eligibility and rate validation
- Cloning programs preserves mentor configuration without participant data

### Mentor Shortlists (`mentor-shortlists`)
- Shortlists reference mentor addresses
- Fetch public profile data for display

## Off-Chain Storage Pattern

The contract commits only hashes; the following data should be stored off-chain:

- Full mentor name and biography
- Detailed expertise descriptions
- Complete policy documents (e.g., cancellation, refund policies)
- Full review text and engagement details
- Supporting credentials and certificates

**Off-chain storage recommendation**:
- Use IPFS, Arweave, or encrypted database
- Store with the hash committed on-chain for verification
- Implement client-side decryption if sensitive

## Deployment

1. Build: `cargo build --release --target wasm32-unknown-unknown`
2. Deploy via Stellar CLI: `stellar contract deploy --wasm target/wasm32-unknown-unknown/release/mentor_profiles.wasm`
3. Initialize: Call `init()` with admin and backend signer addresses
4. Verify: Query `get_admin()` and `get_backend_signer()` for confirmation

## Future Enhancements

- Batch rating submission (multiple ratings per transaction)
- Custom reputation calculation strategies
- Time window (schedule) support for availability
- Language proficiency levels
- Integration with identity/verification contracts
