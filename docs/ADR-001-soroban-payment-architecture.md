# ADR-001: Soroban Payment Architecture

## Status

Proposed

## Context

ChainVerse Academy's payment foundation is migrating from an EVM-native Solidity prototype (World Chain) to Stellar/Soroban. The current Solidity MVP includes:

- **ChainVerseCourseRegistry**: Course ownership, metadata, pricing, and approval
- **ChainVerseMarketplace**: Enrollment ownership, purchases, fees, and completion
- **ChainVerseCertificate**: Non-transferable course-completion NFT
- **ChainVerseRewardToken**: Capped token minted by authorized reward contracts

The current Solidity architecture uses:
- Native token (ETH/Worldcoin) for course purchases
- Platform fee in basis points (max 2000 bps / 20%)
- Pull-based instructor earnings withdrawal
- Admin-controlled course approval and completion verification

Payment execution, accounting, storage, authorization, and event contributors need one frozen contract before parallel work begins. Implementing before agreeing on these boundaries risks incompatible storage layouts, error codes, and payment semantics.

## Decision

We will define the canonical payment lifecycle, public API, storage schema, authorization rules, and accounting invariants for the Stellar payment contract.

## Consequences

### Payment Lifecycle

1. **Initialization**: Contract initialized with admin, token, fee percent, and refund window
2. **Course Configuration**: Admin configures courses with prices and supported Stellar assets
3. **Asset Configuration**: Admin configures supported Stellar assets (XLM and issued assets via SAC addresses)
4. **Payment Execution**: Students pay for courses using configured Stellar assets
5. **Fee Calculation**: Platform fees calculated and deducted from payments
6. **Revenue Splits**: Remaining amount credited to instructor balance
7. **Refunds**: Admin can issue refunds within the refund window
8. **Withdrawals**: Instructors can withdraw their earnings

### Key Differences from Solidity Prototype

| Aspect | Solidity (Current) | Soroban (Target) |
|--------|-------------------|------------------|
| Native Token | ETH/Worldcoin | XLM (via SAC) |
| Asset Support | Single native token | Multiple Stellar assets |
| Storage | EVM storage slots | Persistent/Instance storage |
| Authorization | msg.sender + roles | require_auth() |
| Events | EVM events | Soroban events |
| Replay Protection | Transaction nonce | Soroban host replay protection |
| Idempotency | Transaction-level | Business-level (payment ID) |

### Public API

#### Initialization
- `initialize(admin, token, fee_percent, refund_window_seconds)` - Initialize contract

#### Administration
- `set_fee(caller, fee_percent)` - Set platform fee (admin only)
- `set_refund_window(caller, seconds)` - Set refund window (admin only)
- `configure_course(caller, course_id, price, asset)` - Configure course pricing (admin only)
- `configure_asset(caller, asset, enabled)` - Enable/disable asset (admin only)

#### Payment
- `pay_for_course(student, course_id, amount)` - Pay for a course
- `refund(student, course_id)` - Issue refund (admin only)

#### Withdrawals
- `withdraw_earnings(instructor)` - Withdraw instructor earnings

#### Queries
- `get_instructor_balance(instructor)` - Get instructor balance
- `is_enrolled(student, course_id)` - Check enrollment status
- `get_payment_record(student, course_id)` - Get payment record
- `get_fee_percent()` - Get current fee percent
- `get_refund_window_seconds()` - Get refund window
- `get_course_config(course_id)` - Get course configuration
- `is_asset_enabled(asset)` - Check if asset is enabled
- `version()` - Get contract version

### Storage Schema

#### Storage Keys
```rust
pub enum DataKey {
    Admin,
    Token,
    FeePercent,
    RefundWindowSeconds,
    CourseConfig(Symbol),
    AssetConfig(Address),
    Enrollment(Address, Symbol),
    PaymentRecord(Address, Symbol),
    InstructorBalance(Address),
}
```

#### Storage Selection
- **Instance Storage**: Admin, Token, FeePercent, RefundWindowSeconds (small, frequently accessed)
- **Persistent Storage**: CourseConfig, AssetConfig, Enrollment, PaymentRecord, InstructorBalance (large, long-lived)

#### TTL Policy
- Minimum TTL: 4096 ledgers
- Maximum TTL: 100,000 ledgers
- All persistent storage extended on write

### Authorization Matrix

| Role | Methods | Authorization |
|------|---------|---------------|
| Student | `pay_for_course` | `student.require_auth()` |
| Instructor | `withdraw_earnings` | `instructor.require_auth()` |
| Admin | `set_fee`, `set_refund_window`, `configure_course`, `configure_asset`, `refund` | `admin.require_auth()` |
| Anyone | `get_*`, `is_*`, `version` | No auth required |

### Error Discriminants

```rust
pub enum ContractError {
    AlreadyInitialized = 1,
    NotAdmin = 2,
    NotInitialized = 3,
    InvalidFee = 4,
    CourseNotFound = 5,
    CourseInactive = 6,
    AlreadyEnrolled = 7,
    NotEnrolled = 8,
    PaymentFailed = 9,
    RefundWindowExpired = 10,
    InsufficientBalance = 11,
    TransferFailed = 12,
    InvalidAmount = 13,
    InvalidToken = 14,
    UnauthorizedCaller = 15,
    AssetNotEnabled = 16,
    CourseAlreadyConfigured = 17,
}
```

### Event Topics/Payloads

| Topic | Payload | Description |
|-------|---------|-------------|
| `PYMT_RCD` | (student, course_id, amount, asset, instructor, payment_id) | Payment recorded |
| `RFND_ISS` | (student, course_id, amount) | Refund issued |
| `FEE_SET` | (fee_percent,) | Fee updated |
| `WTHDW` | (instructor, amount) | Withdrawal processed |
| `CRSE_CFG` | (course_id, price, asset) | Course configured |
| `ASSET_CFG` | (asset, enabled) | Asset configured |

### Payment ID Generation

- Payment ID is derived from `(student, course_id)` tuple
- Duplicate payment for same student/course combination is rejected
- Business-level idempotency separate from Soroban host replay protection
- This differs from Solidity where transaction nonces provide replay protection

### Fee Calculation

- Fee calculated as: `fee = (amount * fee_percent) / 10000`
- Integer rounding: truncation (floor division)
- Maximum fee: 2000 basis points (20%)
- Fee deducted from gross payment amount
- Same formula as Solidity prototype

### Invariants

#### Token Custody
- Contract holds tokens until withdrawal or refund
- Total tokens in contract >= sum of all instructor balances
- Refunds return exact amount paid
- Stellar Asset Contract (SAC) used for all token transfers

#### Revenue Splits
- `instructor_amount = amount - fee`
- `fee + instructor_amount = amount`
- No rounding errors in splits

#### Enrollment
- One enrollment per student per course
- Enrollment created on successful payment
- Enrollment removed on refund

#### Withdrawals
- Instructor can only withdraw available balance
- Balance zeroed before transfer
- Transfer amount equals previous balance

## Edge Cases & Failure Scenarios

1. **Same asset used by many courses**: Supported via per-course configuration
2. **Instructor and treasury share an address**: Allowed, no special handling
3. **Course price or asset changes during transaction**: Uses configuration at payment time
4. **Duplicate payment ID with identical arguments**: Rejected by enrollment check
5. **Duplicate payment ID with different arguments**: Not possible (ID derived from student+course)
6. **Contract upgrade after records exist**: Storage schema designed for backward compatibility
7. **Error/event changes breaking downstream**: Stable discriminants and schemas documented
8. **Stellar asset contract interaction failures**: Proper error handling for SAC transfers
9. **Soroban host replay protection vs business idempotency**: Separated concerns

## Migration Notes

### From Solidity to Soroban

1. **Token Handling**: Replace native ETH with Stellar Asset Contract (SAC) addresses
2. **Storage**: Map Solidity mappings to Soroban persistent/instance storage
3. **Authorization**: Replace `msg.sender` checks with `require_auth()` calls
4. **Events**: Convert Solidity events to Soroban event publishing
5. **Roles**: Replace OpenZeppelin AccessControl with direct admin checks
6. **Reentrancy**: Not needed in Soroban (single-threaded execution)

### Preserved Patterns

1. **Fee Calculation**: Same basis point formula
2. **Withdrawal Pattern**: Same pull-based earnings withdrawal
3. **Course Configuration**: Same approval and pricing model
4. **Error Handling**: Similar error discriminants

## References

- Issue #913: [Payments 1A] Specify the Soroban payment architecture, API, storage, and invariants
- Stellar/Soroban documentation
- Existing Solidity contracts: ChainVerseMarketplace.sol, ChainVerseCourseRegistry.sol
- OpenZeppelin AccessControl pattern
