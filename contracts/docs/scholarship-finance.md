# Scholarship Finance Contract

## Overview

The `scholarship-finance` contract provides financial operations for the scholarship system, implementing issues #1106, #1107, #1108, and #1109:

- **#1106**: Transparent fee calculation (platform + network fees)
- **#1107**: Refund and returned payment processing
- **#1108**: Recovery and clawback management
- **#1109**: Sponsor financial statement generation

All operations are privacy-minimized: no PII is stored on-chain. Only addresses, amounts, timestamps, status enums, and cryptographic proofs are retained.

## Contract Dependencies

- `scholarship-core`: Program lifecycle and metadata
- `scholarship-disbursements`: Payment intents and wallet verification
- `scholarship-programs`: Award budgets and application windows
- `shared`: Signing domain and envelope versioning

## Data Model

### FeeConfig
```rust
pub struct FeeConfig {
    pub program_id: BytesN<32>,
    pub platform_fee: FeeBasis,
    pub network_fee_estimate: i128,
    pub fee_currency: Symbol,
    pub version: u32,
    pub updated_at: u64,
    pub updated_by: Address,
}
```

### FeeBasis (versioned, supports preview)
```rust
pub enum FeeBasis {
    Percentage(u32),                    // basis points, e.g., 250 = 2.5%
    Fixed(i128),                        // fixed amount per transaction
    PercentageWithCap { basis_points: u32, cap: i128 }, // capped percentage
}
```

### RefundRecord
```rust
pub struct RefundRecord {
    pub id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub intent_id: BytesN<32>,
    pub recipient: Address,
    pub original_amount: i128,
    pub refund_amount: i128,
    pub fee_refunded: i128,
    pub reason: RefundReason,
    pub status: RefundStatus,
    pub initiated_at: u64,
    pub initiated_by: Address,
    pub completed_at: u64,
    pub tx_hash: Option<BytesN<32>>,
}
```

### RecoveryRecord
```rust
pub struct RecoveryRecord {
    pub id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub intent_id: BytesN<32>,
    pub recipient: Address,
    pub original_amount: i128,
    pub recovery_amount: i128,
    pub reason: RecoveryReason,
    pub legal_basis: String,          // required, max 500 chars
    pub status: RecoveryStatus,
    pub initiated_at: u64,
    pub initiated_by: Address,
    pub completed_at: u64,
    pub tx_hash: Option<BytesN<32>>,
}
```

### FinancialStatement
```rust
pub struct FinancialStatement {
    pub id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub sponsor_id: BytesN<32>,
    pub period_start: u64,
    pub period_end: u64,
    pub currency: Symbol,
    pub contributions: i128,
    pub commitments: i128,
    pub payments: i128,
    pub fees: i128,
    pub refunds: i128,
    pub recoveries: i128,
    pub remaining_balance: i128,
    pub generated_at: u64,
    pub generated_by: Address,
    pub version: u32,
}
```

## Events

| Event | Topic | Data |
|-------|-------|------|
| Fee configuration | `FEECFG` | `(program_id, version)` |
| Refund initiated | `REFUNDNW` | `(refund_id, program_id, intent_id, amount, reason)` |
| Refund completed | `REFUNDCM` | `(refund_id, tx_hash)` |
| Recovery initiated | `RECVRYNW` | `(recovery_id, program_id, intent_id, amount, reason)` |
| Recovery completed | `RECVRYCM` | `(recovery_id, tx_hash)` |
| Statement generated | `STMTGEN` | `(statement_id, program_id, period_start, period_end)` |

## TTL Policy

All persistent records use:
- `RECORD_MIN_TTL = 3,110,400` ledgers (~1 year at 6s ledgers)
- `RECORD_MAX_TTL = 6,220,800` ledgers (~2 years)

## Authorization

- **Admin-only**: `configure_fees`, `initiate_refund`, `complete_refund`, `initiate_recovery`, `complete_recovery`
- **Sponsor/Admin**: `generate_statement` (requires sponsor_id match or admin)
- **Public read**: `get_fee_config`, `calculate_fees`, `get_refund`, `get_recovery`, `get_statement`, `get_program_financials`

## Fee Calculation (#1106)

Fees are configured per-program with versioning. The `calculate_fees` function is a pure preview:
- Input: `program_id`, `gross_amount`
- Output: `(platform_fee, network_fee, total_fee, net_amount)`
- No state changes; safe for UI previews
- Fee basis and rounding are versioned in `FeeConfig.version`
- Fee revenue is separately accounted in `ProgramFinancials.total_fees_collected`

### Rounding Behavior
- Percentage fees: `floor(gross_amount * basis_points / 10000)`
- Proportional fee refunds: same formula applied to refund amount

## Refunds (#1107)

Refunds are initiated by authorized callers and require:
- Valid executed `DisbursementIntent`
- Refund amount ≤ original payment
- Proportional fee refund calculation
- Original entries reversed, not deleted

### Refund Flow
1. `initiate_refund()` → creates `RefundRecord` with `status = Pending`
2. Off-chain execution (Stellar payment)
2. `complete_refund(tx_hash)` → marks `Completed`, records tx hash
3. `ProgramFinancials` updated: `total_refunded += amount`, `total_fees_collected -= fee_refunded`

### Refund Reasons
```rust
enum RefundReason {
    Overpayment,
    SponsorCancellation,
    RecipientIneligible,
    DuplicatePayment,
    NetworkRejection,
    ProgramClosed,
}
```

## Recoveries & Clawbacks (#1108)

Recoveries are initiated for specific reasons with recorded legal basis:
- Never silently debits a wallet
- Reason and legal basis are mandatory
- Collected amounts reconcile to open claims

### Recovery Flow
1. `initiate_recovery()` → creates `RecoveryRecord` with `status = Pending`
2. Off-chain execution
2. `complete_recovery(tx_hash)` → marks `Completed`
3. `ProgramFinancials` updated: `total_recovered += amount`

### Recovery Reasons
```rust
enum RecoveryReason {
    Fraud,
    MilestoneFailure,
    Withdrawal,
    PolicyViolation,
    ComplianceHold,
}
```

## Financial Statements (#1109)

Statements are generated per-program, per-period:
- Reconcile to ledger entries via event log
- Use consistent currency metadata from program
- Large exports: statement generation is a single contract call; bulk export runs off-chain by paginating statements

### Statement Fields
| Field | Source |
|-------|--------|
| `contributions` | Total funded to program |
| `commitments` | Pending disbursements |
| `payments` | Executed disbursements |
| `fees` | Platform fees collected |
| `refunds` | Refunded amounts |
| `recoveries` | Clawback amounts |
| `remaining_balance` | `contributions - payments - fees + refunds + recoveries` |

## Migration & Upgrades

- `FeeConfig.version` increments on each update for audit trail
- `FinancialStatement.version` tracks schema version
- Adding new fee types: extend `FeeBasis` enum, update `calculate_fees`
- Adding new statement fields: increment `FinancialStatement.version`, handle missing fields in indexer

## Operational Notes

1. **Fee previews**: Always call `calculate_fees` before `create_intent` to show gross/net
2. **Refund timing**: Refunds can be initiated for any executed intent; no time limit
3. **Recovery audit**: `legal_basis` field must reference policy/contract section
4. **Statement frequency**: Sponsors should generate monthly/quarterly; no automatic generation
5. **Currency**: All amounts in program's currency (`Symbol`); no on-chain conversion

## Security Considerations

- All arithmetic uses checked operations (`checked_add`, `checked_mul`, etc.)
- Admin authorization required for all state-changing operations
- Fee configuration validated at write time (basis points ≤ 10000, caps ≥ 0)
- Refund/recovery amounts validated against original intent amounts
- Network ID bound at initialization prevents testnet/mainnet replay