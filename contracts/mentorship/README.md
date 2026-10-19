# Mentorship Contract

Privacy-minimized Soroban smart contract for managing 1-on-1 mentorship engagements with smart-contract-backed payments on ChainVerse.

## Features

This contract implements four key mentorship capabilities:

### 1. Guardian Consent Management (MENTOR-047)
- **Invitation & Verification**: Model guardian relationships with verified references
- **Scoped Consent**: Grant consent for specific engagements with optional expiry
- **Material Changes**: Track changes requiring re-consent
- **Revocation**: Guardians can revoke consent, automatically terminating engagement
- **Privacy**: Only hashes and status on-chain; private content off-chain

### 2. Pause and Resume (MENTOR-049)
- **Policy-Compliant Pause Requests**: Submit pause with reason and effective dates
- **Reason Visibility**: Track pause reasons (scheduled break, health, etc.) via hashes
- **Approval Rules**: Configurable mutual or coordinator approval
- **Scheduling Impact**: Track effective dates and optional resume dates
- **Expiry Handling**: Automatic expiry logic for pause requests

### 3. Engagement Completion/Termination (MENTOR-050)
- **Completion**: Mark successful completion or early mutual termination
- **Policy Termination**: Admin-initiated termination for policy violations
- **Safeguarding**: Special termination type for safeguarding concerns
- **Inactivity**: Automatic termination trigger based on configurable threshold
- **Structured Outcomes**: Record final session count, reason, and settlement status

### 4. Matching Feedback Loop (MENTOR-043)
- **Private Feedback**: Collect feedback via content hashes (off-chain storage)
- **Feedback Types**: Recommendation relevance, declined proposals, match quality
- **Reference Linking**: Optional reference to engagement or proposal
- **Privacy-First**: Only metadata and hashes stored on-chain

## Data Model

### Engagement Lifecycle States
- `PendingConsent`: Awaiting guardian consent
- `Active`: Engagement in progress
- `Paused`: Temporarily paused
- `Completed`: Successfully finished
- `Terminated`: Ended early (mutual or policy)
- `SafeguardingTermination`: Ended due to safeguarding concerns

### Key Types
- **Engagement**: Core mentorship record with participants, status, payment info
- **GuardianConsent**: Consent record with terms hash, expiry, and validity
- **PauseRequest**: Pause details with reason, dates, and approval status
- **MatchingFeedback**: User feedback with type and content hash
- **TerminationOutcome**: Final outcome with reason and settlement info

## Privacy Design

This contract follows a **privacy-minimized** approach:
- ✅ On-chain: Addresses, amounts, status, timestamps, public attestations
- ✅ On-chain: Cryptographic hashes of agreements, reasons, feedback
- ❌ Off-chain: Personal details, private content, sensitive reasons
- ❌ Off-chain: Actual feedback text, agreement content, correspondence

## Usage

### Initialize Contract
```rust
contract.init(admin_address);
```

### Create Engagement
```rust
let engagement_id = contract.create_engagement(
    mentor,
    mentee,
    agreement_hash,      // SHA-256 of off-chain agreement
    payment_per_session, // In stroops
    payment_token,
    planned_sessions,
    requires_guardian_consent,
);
```

### Grant Guardian Consent
```rust
contract.grant_guardian_consent(
    guardian,
    engagement_id,
    terms_hash,
    Some(expiry_timestamp),
);
```

### Request Pause
```rust
contract.request_pause(
    requester,
    engagement_id,
    PauseReason::ScheduledBreak,
    reason_hash,
    effective_date,
    Some(resume_date),
);
```

### Record Session
```rust
contract.record_session(participant, engagement_id);
```

### Submit Feedback
```rust
let feedback_id = contract.submit_feedback(
    user,
    feedback_hash,
    FeedbackType::MatchQuality,
    Some(engagement_id),
);
```

### Terminate Engagement
```rust
contract.terminate_engagement(
    caller,
    engagement_id,
    TerminationReason::Mutual,
    outcome_hash,
);
```

## Authorization Model

- **Admin**: Set configuration, policy/safeguarding terminations
- **Participants** (Mentor/Mentee): Create engagements, record sessions, request pause/resume
- **Guardians**: Grant/revoke consent, re-consent after material changes
- **Anyone**: Submit feedback, trigger inactivity termination (if threshold met)

## Events

The contract emits events for all key actions:
- `ENG_NEW`: New engagement created
- `CONSENT`: Guardian consent granted
- `CNS_REVK`: Consent revoked
- `CNS_RENW`: Re-consent required
- `ENG_ACT`: Engagement activated
- `PAUSE_RQ`: Pause requested
- `PAUSE_OK`: Pause approved
- `PAUSED`: Engagement paused
- `RESUMED`: Engagement resumed
- `TERM`: Engagement terminated
- `COMPLETE`: Engagement completed
- `FEEDBACK`: Feedback submitted
- `SESSION`: Session recorded

## Configuration

Admins can configure:
- **Inactivity Threshold**: Time (seconds) before engagement can be terminated for inactivity
- **Pause Approval Policy**: Whether pause requests require mutual approval

## Testing

Run tests with:
```bash
cargo test
```

## Integration

This contract is designed to integrate with:
- **Escrow Contract**: For payment handling and milestone releases
- **Certificate Contract**: For issuing completion certificates
- **Off-chain Backend**: For storing private content referenced by hashes

## Security Considerations

1. **Authorization**: All state-changing operations require proper authentication
2. **State Validation**: Status transitions are strictly enforced
3. **Guardian Protection**: Minors cannot bypass guardian consent requirements
4. **Privacy**: Sensitive data never stored on-chain
5. **Immutable History**: Termination outcomes preserved for audit

## License

Part of the ChainVerse on-chain contracts suite.
