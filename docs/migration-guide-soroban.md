# Migration Guide: Solidity to Soroban

## Overview

This guide documents the migration of ChainVerse Academy's payment system from the EVM-native Solidity prototype to Stellar/Soroban.

## Current Solidity Architecture

### Contracts

1. **ChainVerseCourseRegistry**
   - Course ownership, metadata, pricing, approval
   - Uses OpenZeppelin AccessControl for roles
   - Stores courses in mappings

2. **ChainVerseMarketplace**
   - Enrollment ownership, purchases, fees, completion
   - Native token (ETH/Worldcoin) payments
   - Pull-based instructor earnings

3. **ChainVerseCertificate**
   - Soulbound ERC-721 completion certificates

4. **ChainVerseRewardToken**
   - Capped ERC-20 completion rewards

### Key Patterns

- **Fee Calculation**: `(amount * platformFeeBps) / 10_000`
- **Authorization**: `msg.sender` + role-based access control
- **Storage**: Solidity mappings
- **Events**: Solidity events
- **Replay Protection**: Transaction nonces

## Target Soroban Architecture

### Contract Structure

Single payment contract with the following modules:

```
contracts/
├── payment/
│   ├── src/
│   │   ├── lib.rs          # Main contract
│   │   ├── storage.rs      # Storage types and functions
│   │   ├── errors.rs       # Error definitions
│   │   ├── events.rs       # Event definitions
│   │   ├── fee.rs          # Fee calculation
│   │   └── test.rs         # Tests
│   └── Cargo.toml
└── chainverse-types/
    ├── src/
    │   └── lib.rs          # Shared types
    └── Cargo.toml
```

### Key Differences

| Aspect | Solidity | Soroban |
|--------|----------|---------|
| Token | Native ETH | Stellar Asset Contract (SAC) |
| Storage | Mappings | Persistent/Instance storage |
| Authorization | msg.sender + roles | require_auth() |
| Events | Solidity events | Soroban events |
| Replay Protection | Transaction nonces | Soroban host + business-level |
| Roles | OpenZeppelin AccessControl | Direct admin checks |
| Reentrancy | ReentrancyGuard | Not needed (single-threaded) |

## Migration Steps

### 1. Token Handling

**Solidity:**
```solidity
function purchaseCourse(uint256 courseId) external payable {
    // msg.value is the payment amount
}
```

**Soroban:**
```rust
pub fn pay_for_course(
    env: Env,
    student: Address,
    course_id: Symbol,
    amount: i128,
) -> Result<(), ContractError> {
    let token = read_token(&env);
    let token_client = soroban_sdk::token::Client::new(&env, &token);
    token_client.transfer(&student, &env.current_contract_address(), &amount);
}
```

### 2. Storage Mapping

**Solidity:**
```solidity
mapping(uint256 courseId => mapping(address student => bool enrolled)) public isEnrolled;
mapping(address instructor => uint256 balance) public instructorEarnings;
```

**Soroban:**
```rust
pub enum DataKey {
    Enrollment(Address, Symbol),
    InstructorBalance(Address),
}

pub fn is_enrolled(env: &Env, student: &Address, course_id: &Symbol) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::Enrollment(student.clone(), course_id.clone()))
}
```

### 3. Authorization

**Solidity:**
```solidity
function setFee(uint16 newFeeBps) external onlyRole(DEFAULT_ADMIN_ROLE) {
    // ...
}
```

**Soroban:**
```rust
pub fn set_fee(env: Env, caller: Address, fee_percent: u32) -> Result<(), ContractError> {
    let admin = read_admin(&env)?;
    if caller != admin {
        return Err(ContractError::NotAdmin);
    }
    caller.require_auth();
    // ...
}
```

### 4. Events

**Solidity:**
```solidity
event CoursePurchased(
    uint256 indexed courseId,
    address indexed student,
    address indexed instructor,
    uint256 price,
    uint256 platformFee
);
```

**Soroban:**
```rust
pub fn payment_recorded(
    env: &Env,
    student: Address,
    course_id: Symbol,
    amount: i128,
    asset: Address,
    instructor: Address,
    payment_id: Symbol,
) {
    env.events().publish(
        (symbol_short!("PYMT_RCD"),),
        (student, course_id, amount, asset, instructor, payment_id),
    );
}
```

### 5. Fee Calculation

**Solidity:**
```solidity
uint256 platformFee = (msg.value * course.platformFeeBps) / 10_000;
```

**Soroban:**
```rust
pub fn calculate_fee(amount: i128, fee_percent: u32) -> Result<i128, ContractError> {
    if fee_percent > MAX_FEE_BASIS_POINTS {
        return Err(ContractError::InvalidFee);
    }
    Ok((amount * fee_percent as i128) / FEE_DENOMINATOR as i128)
}
```

## New Features

### 1. Multi-Asset Support

Soroban contract supports multiple Stellar assets per course:

```rust
pub fn configure_course(
    env: Env,
    caller: Address,
    course_id: Symbol,
    price: i128,
    asset: Address,
) -> Result<(), ContractError>;
```

### 2. Business-Level Idempotency

Payment ID derived from `(student, course_id)` tuple:

```rust
pub struct PaymentRecord {
    pub student: Address,
    pub course_id: Symbol,
    pub amount: i128,
    pub asset: Address,
    pub paid_at: u64,
    pub payment_id: Symbol,
}
```

### 3. Refund Window

Configurable refund window (not available in Solidity MVP):

```rust
pub fn set_refund_window(
    env: Env,
    caller: Address,
    seconds: u64,
) -> Result<(), ContractError>;
```

## Testing

### Solidity (Hardhat)

```javascript
describe("Marketplace", function () {
  it("Should allow course purchase", async function () {
    await marketplace.connect(student).purchaseCourse(courseId, { value: price });
  });
});
```

### Soroban

```rust
#[test]
fn test_pay_for_course_success() {
    let env = Env::default();
    let (admin, student, _, token) = setup_contract(&env);

    // Configure asset and course
    PaymentContract::configure_asset(env.clone(), admin.clone(), token.clone(), true).unwrap();
    PaymentContract::configure_course(env.clone(), admin.clone(), course_id, price, token.clone()).unwrap();

    // Pay for course
    PaymentContract::pay_for_course(env.clone(), student.clone(), course_id, price).unwrap();

    assert!(PaymentContract::is_enrolled(env, student, course_id));
}
```

## Deployment

### Solidity

```bash
npm run build
npx hardhat deploy --network worldchain
```

### Soroban

```bash
stellar contract build
stellar contract deploy --network testnet
```

## References

- ADR-001: Soroban Payment Architecture
- Stellar/Soroban Documentation
- Solidity Contracts: ChainVerseMarketplace.sol, ChainVerseCourseRegistry.sol
