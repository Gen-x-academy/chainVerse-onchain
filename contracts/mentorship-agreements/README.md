# Mentorship Agreements Contract

Privacy-minimized Soroban smart contract for 1-on-1 mentorship with smart-contract-backed payments on ChainVerse.

## Overview

This contract implements the on-chain primitives for ChainVerse's mentorship capability, covering:

- **MENTOR-044**: Bidirectional user blocking for matching prevention
- **MENTOR-045**: Mentorship engagement agreement creation
- **MENTOR-046**: Participant agreement acceptance with replay protection
- **MENTOR-048**: Versioned agreement amendments with approval workflow

## Privacy Design

Following ChainVerse's privacy-first architecture, this contract stores **only**:

- Agreement structure and status
- Cryptographic hashes of private content
- Participant addresses
- Financial terms (amounts, tokens)
- Timestamps and public attestations

All private content (program details, goals, evidence, conduct policies) is stored **off-chain** with only content-addressed hashes committed on-chain.

## Features

### User Blocking (MENTOR-044)

```rust
// Block a user from matching
block_user(blocker: Address, blocked: Address, reason_hash: Option<BytesN<32>>)

// Unblock a user
unblock_user(blocker: Address, blocked: Address)

// Check if matching is allowed (bidirectional)
check_matching_allowed(user1: Address, user2: Address) -> MatchingAllowed
```

**Safeguarding**: Bidirectional blocks prevent either party from discovering, proposing, or being matched with the other.

### Agreement Creation (MENTOR-045)

```rust
create_agreement(
    mentor: Address,
    mentee: Address,
    program_hash: BytesN<32>,          // Off-chain program details
    scope_hash: BytesN<32>,            // Off-chain goals and scope
    start_time: u64,
    end_time: u64,
    cadence_seconds: u64,              // Session frequency
    price_per_session: i128,           // Payment amount
    token: Address,                     // Payment token
    cancellation_policy_hash: BytesN<32>,
    conduct_hash: BytesN<32>,
    evidence_hash: BytesN<32>,
    terms_version: u32
) -> u64  // agreement_id
```

**Validations**:
- Both parties must authenticate
- Participants must be different addresses
- No blocking relationship between participants
- Valid time range (start < end)
- Non-negative price
- Positive cadence

### Agreement Acceptance (MENTOR-046)

```rust
accept_agreement(
    actor: Address,
    agreement_id: u64,
    agreement_hash: BytesN<32>,  // Hash of accepted terms
    expected_nonce: u64           // For replay protection
)
```

**Features**:
- Explicit acceptance with actor authentication
- Agreement hash commitment
- Replay-resistant intent via nonce
- Auto-activation when both parties accept
- Immutable acceptance history

**Nonce Management**:
```rust
get_user_nonce(user: Address) -> u64
```

Each acceptance increments the user's nonce, preventing replay attacks.

### Agreement Amendments (MENTOR-048)

```rust
// Propose changes
propose_amendment(
    proposer: Address,
    agreement_id: u64,
    new_cadence: Option<u64>,
    new_scope_hash: Option<BytesN<32>>,
    new_start_time: Option<u64>,
    new_end_time: Option<u64>,
    new_price: Option<i128>,
    new_cancellation_hash: Option<BytesN<32>>,
    impact_hash: BytesN<32>  // Off-chain impact preview
) -> u64  // amendment_id

// Approve proposed amendment
approve_amendment(approver: Address, amendment_id: u64)

// Reject proposed amendment
reject_amendment(rejector: Address, amendment_id: u64)

// Activate approved amendment
activate_amendment(caller: Address, amendment_id: u64)
```

**Workflow**:
1. Participant proposes amendment (proposer auto-approves)
2. Other participant(s) approve or reject
3. If fully approved, any participant can activate
4. **Old terms govern until activation** (acceptance criteria)
5. Rejected amendments leave agreement intact
6. History is immutable

## Agreement Lifecycle

```
Created (Pending)
    ↓ both accept
Active
    ↓ complete or cancel
Completed / Cancelled
```

### Status Management

```rust
cancel_agreement(caller: Address, agreement_id: u64)
complete_agreement(caller: Address, agreement_id: u64)
```

## Data Types

### Agreement
- ID, participants, status
- Program/scope/conduct/evidence hashes (off-chain content)
- Time range, cadence, price, token
- Terms version, timestamps

### Acceptance
- Actor, agreement ID, timestamp
- Agreement hash at acceptance time
- Terms version accepted
- Nonce (replay protection)

### Amendment
- Proposed changes (optional fields)
- Approval tracking (mentor/mentee)
- Status lifecycle
- Impact preview hash (off-chain)
- Activation timestamp

### Block
- Blocker/blocked addresses
- Timestamp
- Optional reason hash (privacy-preserving)

## Events

All state changes emit events:

- `init`: Contract initialized
- `usr_block`: User blocked
- `usr_unblk`: User unblocked
- `agmt_new`: Agreement created
- `agmt_acpt`: Agreement accepted by party
- `agmt_stat`: Agreement status changed
- `amnd_prop`: Amendment proposed
- `amnd_aprv`: Amendment approved
- `amnd_rjct`: Amendment rejected
- `amnd_actv`: Amendment activated

## Security

### Authorization
- All mutations require caller authentication via `require_auth()`
- Participants must be agreement parties for acceptance/amendments
- Admin controls for initialization

### Validation
- Replay protection via user nonces
- Same-participant prevention
- Block relationship enforcement
- Time range validation
- Material change requirements for amendments

### Privacy
- Private content stored off-chain
- Only content hashes and public attestations on-chain
- Optional reason hashes for blocks

## Testing

Comprehensive test suite covers:
- Initialization
- Blocking workflows
- Agreement creation and validation
- Acceptance with replay protection
- Amendment proposal, approval, rejection, activation
- Error cases and authorization

Run tests:
```bash
cargo test -p mentorship-agreements
```

## Integration

### Off-chain Components Required

1. **Content Storage**: IPFS, Arweave, or private database for program details, goals, evidence requirements
2. **Hash Generation**: Client-side hashing of private content before contract calls
3. **Matching Service**: Discovery/recommendation system that queries `check_matching_allowed` before suggesting matches
4. **Payment Integration**: Escrow contract integration for `price_per_session` settlements
5. **Notification Service**: Listen to events for acceptance, amendment workflows

### Example Flow

```
1. Users agree off-chain on mentorship terms
2. Hash private content (program, scope, conduct, etc.)
3. Call create_agreement with hashes
4. Each party calls accept_agreement with nonce
5. Contract auto-activates when both accept
6. For changes: propose_amendment → approve → activate
7. Payment contract references agreement for session billing
```

## Deployment

Standard Soroban deployment:

```bash
# Build
soroban contract build

# Deploy
soroban contract deploy \
    --wasm target/wasm32-unknown-unknown/release/mentorship_agreements.wasm \
    --network testnet

# Initialize
soroban contract invoke \
    --id <CONTRACT_ID> \
    --network testnet \
    -- init \
    --admin <ADMIN_ADDRESS>
```

## Acceptance Criteria

✅ **MENTOR-044**: Bidirectional blocks prevent matching  
✅ **MENTOR-045**: Agreement captures all specified fields with privacy hashes  
✅ **MENTOR-046**: Acceptance with actor, hash, terms version, timestamp, nonce  
✅ **MENTOR-048**: Old terms govern until activation, material changes require all approvals, rejected amendments leave agreement intact, history immutable  

## License

See repository root for license information.
