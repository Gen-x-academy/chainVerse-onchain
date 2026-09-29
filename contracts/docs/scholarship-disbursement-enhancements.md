# Scholarship Disbursement Enhancements Contract

## Overview

The `scholarship-disbursement-enhancements` contract extends the base `scholarship-disbursements` contract with advanced disbursement features (issues #1098, #1099, #1100, #1101):

- **#1098**: Support Stellar asset configuration (native/issued assets, network, issuer, decimals, trustlines)
- **#1099**: Execute scheduled scholarship payments (bounded batches, deterministic outcomes)
- **#1100**: Track pending and finalized transactions (state machine, configurable confirmations)
- **#1101**: Handle failed payout recovery (missing trustlines, bad destinations, retry with new envelopes)

## Data Model

### AssetConfig (#1098)
```rust
pub struct AssetConfig {
    pub program_id: BytesN<32>,
    pub asset_type: AssetType,  // Native | Issued
    pub asset_code: String,     // e.g., "XLM", "USDC"
    pub issuer: Option<Address>, // None for native
    pub decimals: u32,
    pub trustline_required: bool,
    pub is_active: bool,
    pub created_at: u64,
    pub updated_at: u64,
}
```

Key features:
- Native assets (XLM): fixed 7 decimals, no issuer, no trustline
- Issued assets: configurable issuer, decimals (1-18), trustline required
- Configuration governed by admin; changes tracked with timestamps
- Unsupported assets fail early at intent creation

### TransactionRecord (#1100)
```rust
pub struct TransactionRecord {
    pub id: BytesN<32>,
    pub intent_id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub recipient: Address,
    pub amount: i128,
    pub asset_code: String,
    pub tx_hash: Option<BytesN<32>>,
    pub state: TransactionState,  // Submitted, Pending, Confirming, Confirmed, Finalized, Failed, Expired, Reversed
    pub confirmations: u32,
    pub required_confirmations: u32,
    pub submitted_at: u64,
    pub confirmed_at: Option<u64>,
    pub finalized_at: Option<u64>,
    pub failure_reason: Option<String>,
    pub retry_count: u32,
    pub envelope_xdr: Option<Bytes>, // For retry with new envelope
}
```

State machine (enforced):
```
Submitted → Pending → Confirming → Confirmed → Finalized
                    ↓
                    Failed → (recovery) → Submitted (retry)
                    ↓
                    Expired
                    ↓
Finalized → Reversed
```

Key features:
- State follows verified network evidence
- Confirmations configurable per transaction
- Payment cannot finalize twice (enforced)
- Retry uses new envelope, same intent

### RecoveryAttempt (#1101)
```rust
pub struct RecoveryAttempt {
    pub id: BytesN<32>,
    pub original_intent_id: BytesN<32>,
    pub transaction_id: BytesN<32>,
    pub failure_reason: String,
    pub recovery_type: RecoveryType,  // TrustlineMissing, BadDestination, InsufficientFunds, NetworkExpiry, NetworkError, Other
    pub status: RecoveryStatus,       // Pending, InProgress, Completed, Failed, RequiresManualIntervention
    pub retry_envelope_xdr: Option<Bytes>,
    pub attempted_at: u64,
    pub completed_at: Option<u64>,
    pub tx_hash: Option<BytesN<32>>,
}
```

Key features:
- Failures preserve award eligibility
- Retries use new transaction envelopes but same intent
- Operators receive actionable diagnostics via failure_reason
- Recovery types categorize failure for automated handling

### BatchExecutionRecord (#1099)
```rust
pub struct BatchExecutionRecord {
    pub id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub intent_ids: Vec<BytesN<32>>,
    pub total_amount: i128,
    pub asset_code: String,
    pub executed_count: u32,
    pub failed_count: u32,
    pub executed_at: u64,
    pub executed_by: Address,
}
```

Key features:
- Bounded batch size (MAX_BATCH_SIZE = 100)
- Only authorized automation executes
- Partial batch outcomes reconcile per intent
- Successful payments record ledger references

## Events

| Event | Topic | Data |
|-------|-------|------|
| Asset configured | `ASSETCFG` | `(program_id, asset_type)` |
| Batch executed | `BATCHEXE` | `(batch_id, program_id, executed, failed)` |
| Transaction created | `TXNEW` | `(tx_id, intent_id, state)` |
| Transaction state changed | `TXSTATE` | `(tx_id, new_state, confirmations)` |
| Recovery initiated | `RECVRYNW` | `(recovery_id, tx_id, recovery_type)` |
| Recovery completed | `RECVRYCM` | `(recovery_id, status)` |

## Authorization

| Operation | Authorization |
|-----------|---------------|
| `configure_asset` | Admin |
| `get_asset_config` | Public |
| `validate_intent_asset` | Public |
| `execute_batch` | Creator/Admin |
| `create_transaction_record` | Creator/Admin |
| `update_transaction_state` | Admin |
| `get_transaction` | Public |
| `initiate_recovery` | Admin |
| `complete_recovery` | Admin |
| `get_recovery_attempt` | Public |

## TTL Policy

All persistent records use:
- `RECORD_MIN_TTL = 3,110,400` (~1 year)
- `RECORD_MAX_TTL = 6,220,800` (~2 years)

## Asset Configuration Rules (#1098)

| Asset Type | Code | Issuer | Decimals | Trustline |
|------------|------|--------|----------|-----------|
| Native | XLM | None | 7 | false |
| Issued | Any (1-12 chars) | Required | 1-18 | true |

Configuration changes are governed by admin; recipients receive trustline guidance via off-chain notification when asset requires trustline.

## Batch Execution (#1099)

```rust
execute_batch(executor, program_id, intent_ids[])
```

- Validates all intents belong to program
- Checks `is_intent_executable` for each (wallet verified, correct epoch)
- Creates `TransactionRecord` for each intent
- Calls `record_execution` on base contract
- Tracks executed/failed counts
- Emits `BATCHEXE` event with results

Max batch size: 100 intents.

## Transaction State Tracking (#1100)

State transitions enforced:
- Submitted → Pending (network accepted)
- Pending → Confirming (first confirmation)
- Confirming → Confirmed (confirmations >= required)
- Confirmed → Finalized (irreversible)
- Any → Failed (network rejection)
- Any → Expired (timeout)
- Finalized → Reversed (refund/recovery)

Payment cannot finalize twice (enforced by `AlreadyExecuted` error).

## Failed Payment Recovery (#1101)

Recovery types:
| Type | Description | Automated Retry |
|------|-------------|-----------------|
| TrustlineMissing | Recipient lacks trustline | Yes (after trustline created) |
| BadDestination | Invalid address | No (manual) |
| InsufficientFunds | Program wallet empty | Yes (after funding) |
| NetworkExpiry | Transaction expired | Yes (new envelope) |
| NetworkError | Transient network issue | Yes (new envelope) |
| Other | Unclassified | Manual review |

Recovery flow:
1. `initiate_recovery()` creates `RecoveryAttempt` with `retry_envelope_xdr`
2. Off-chain worker submits retry envelope
3. `complete_recovery()` with `status = Completed`
4. Original transaction reset to `Submitted`, `retry_count++`
5. Award eligibility preserved throughout

## Migration & Upgrades

- Asset configs: new configs replace old; old intents use config at creation
- Transaction states: new states added to enum (append-only)
- Recovery types: extensible enum
- Batch size: configurable via constant

## Operational Notes

1. **Asset config**: Set before creating intents; changing config doesn't affect existing intents
2. **Batch execution**: Run via authorized cron/automation; monitor `failed_count`
3. **Confirmations**: Set `required_confirmations` based on asset value (3 for XLM, more for issued)
4. **Recovery**: Auto-retry for TrustlineMissing, NetworkExpiry, NetworkError; manual for others
5. **Envelope XDR**: Store for retry; includes all signatures except network sequence

## Security Considerations

- All arithmetic uses checked operations
- Admin authorization for state-changing operations
- State transitions strictly enforced (no skipping)
- Payment cannot finalize twice
- Recovery preserves original intent (no double-pay)
- Retry uses new envelope (prevents replay)