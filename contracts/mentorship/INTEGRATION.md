# Mentorship Contract Integration Guide

## Overview

This document describes how the Mentorship contract integrates with other ChainVerse contracts and off-chain systems to provide complete 1-on-1 mentorship functionality.

## Architecture

```
┌─────────────────┐
│  Off-chain API  │ (Stores private content, manages hashes)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│   Mentorship    │ (Core lifecycle, consent, pause/resume)
│    Contract     │
└────────┬────────┘
         │
    ┌────┴─────┬────────────┬──────────────┐
    ▼          ▼            ▼              ▼
┌────────┐ ┌────────┐ ┌──────────┐ ┌──────────┐
│ Escrow │ │  Token │ │Certificate│ │ Identity │
└────────┘ └────────┘ └──────────┘ └──────────┘
```

## Integration Points

### 1. Payment Flow (Escrow Contract)

**Session Payment Workflow:**

1. **Engagement Creation**: Mentorship contract records payment_per_session and payment_token
2. **Session Escrow**: When session scheduled, create escrow:
   ```rust
   escrow.create_escrow(
       mentee,           // buyer
       mentor,           // seller
       engagement.payment_token,
       engagement.payment_per_session,
       session_expiry
   );
   ```

3. **Session Completion**: After `record_session()`:
   ```rust
   // Off-chain backend triggers
   escrow.release_escrow(mentee, escrow_id);
   ```

4. **Termination Settlement**: On engagement termination:
   - Completed sessions: All escrowed funds released to mentor
   - Pending sessions: Refund to mentee or partial release based on policy

**Key Considerations:**
- One escrow per session OR bulk escrow for multiple sessions
- Pause should freeze new escrow creation
- Termination triggers settlement logic

### 2. Guardian Verification (Identity System)

**Guardian Relationship Verification:**

Off-chain system must verify guardian relationship before allowing consent:

```typescript
// Pseudo-code for off-chain verification
async function verifyGuardianRelationship(
  guardian: Address,
  mentee: Address
): Promise<boolean> {
  // Check government ID, legal documentation, or
  // existing verified relationships in identity system
  
  if (!verified) {
    throw new Error("Guardian relationship not verified");
  }
  
  // Generate consent terms document
  const termsDoc = generateConsentTerms(engagement);
  const termsHash = sha256(termsDoc);
  
  // Store document off-chain
  await storePrivateDocument(termsDoc, termsHash);
  
  // Submit to contract
  await mentorship.grant_guardian_consent(
    guardian,
    engagementId,
    termsHash,
    expiryTimestamp
  );
}
```

### 3. Certificate Issuance

**Completion Certificates:**

When engagement reaches `Completed` status:

```rust
// Listen for completion event
if engagement.status == EngagementStatus::Completed {
    let certificate_metadata = build_certificate_metadata(
        engagement.mentor,
        engagement.mentee,
        engagement.completed_sessions,
        engagement.created_at,
        now
    );
    
    // Mint certificate (soulbound NFT)
    certificate_contract.mint(
        engagement.mentee,
        certificate_hash,
        proof
    );
}
```

### 4. Feedback Collection & Analysis

**Privacy-Preserving Feedback Loop:**

```typescript
// Off-chain feedback submission
async function submitFeedback(
  user: Address,
  feedbackType: FeedbackType,
  feedbackContent: {
    rating?: number;
    text?: string;
    suggestions?: string[];
  },
  referenceId?: number
): Promise<number> {
  // Store feedback privately
  const feedbackDoc = JSON.stringify(feedbackContent);
  const feedbackHash = sha256(feedbackDoc);
  
  await storePrivateFeedback(feedbackDoc, feedbackHash);
  
  // Record hash on-chain
  const feedbackId = await mentorship.submit_feedback(
    user,
    feedbackHash,
    feedbackType,
    referenceId
  );
  
  // Trigger ML pipeline for matching improvement
  await improvementPipeline.processFeedback(feedbackId, feedbackContent);
  
  return feedbackId;
}
```

**Feedback Types Usage:**

- `RecommendationRelevance`: After viewing mentor recommendations
- `DeclinedProposal`: When mentee declines a match proposal
- `MatchQuality`: After engagement completion
- `General`: Platform-wide feedback

### 5. Pause/Resume Workflow

**Scheduling System Integration:**

```typescript
// When pause is requested
async function handlePauseRequest(engagementId: number) {
  const engagement = await mentorship.get_engagement(engagementId);
  const pauseRequest = engagement.active_pause;
  
  if (pauseRequest.requires_approval) {
    // Notify other party for approval
    await notifyPartyForApproval(engagement, pauseRequest);
  }
  
  // Cancel scheduled sessions after effective_date
  await scheduler.cancelSessions(
    engagementId,
    pauseRequest.effective_date,
    pauseRequest.resume_date
  );
  
  // Refund any pre-funded escrows for cancelled sessions
  await refundCancelledSessions(engagementId);
}

// When engagement resumes
async function handleResume(engagementId: number) {
  // Reschedule sessions
  await scheduler.rescheduleEngagement(engagementId);
}
```

### 6. Inactivity Monitoring

**Background Job:**

```typescript
// Run periodically (e.g., daily)
async function checkInactiveEngagements() {
  const activeEngagements = await getActiveEngagements();
  const threshold = await mentorship.get_inactivity_threshold();
  const now = Date.now() / 1000;
  
  for (const engagement of activeEngagements) {
    if (now - engagement.last_activity_at > threshold) {
      // Trigger inactivity termination
      await mentorship.terminate_engagement(
        systemAddress,
        engagement.id,
        TerminationReason.Inactivity,
        generateOutcomeHash(engagement)
      );
      
      // Handle cleanup
      await handleTerminationCleanup(engagement);
    }
  }
}
```

## Event Listeners

### Essential Event Handlers

```typescript
// Listen to key events
mentorship.on('ENG_NEW', async (data) => {
  const { engagementId, mentor, mentee, requiresConsent } = data;
  
  if (requiresConsent) {
    // Notify guardian invitation workflow
    await notifyGuardianInvitation(engagementId, mentee);
  } else {
    // Initialize scheduling system
    await scheduler.initializeEngagement(engagementId);
  }
});

mentorship.on('CONSENT', async (data) => {
  const { engagementId } = data;
  // Activate engagement in scheduling system
  await scheduler.activateEngagement(engagementId);
});

mentorship.on('CNS_REVK', async (data) => {
  const { engagementId } = data;
  // Cancel all future sessions
  await scheduler.cancelAllSessions(engagementId);
  // Process refunds
  await processConsentRevocationRefunds(engagementId);
});

mentorship.on('PAUSED', async (data) => {
  const { engagementId } = data;
  await scheduler.pauseEngagement(engagementId);
});

mentorship.on('COMPLETE', async (data) => {
  const { engagementId, totalSessions } = data;
  // Issue certificate
  await issueCertificate(engagementId);
  // Finalize payments
  await finalizePayments(engagementId);
  // Request feedback
  await requestCompletionFeedback(engagementId);
});

mentorship.on('TERM', async (data) => {
  const { engagementId, reason } = data;
  await handleTermination(engagementId, reason);
});
```

## Off-Chain Data Storage

### Required Off-Chain Documents

1. **Engagement Agreements** (agreement_hash)
   - Mentorship objectives
   - Session schedule
   - Payment terms
   - Cancellation policy

2. **Guardian Consent Terms** (terms_hash)
   - Engagement details
   - Safety measures
   - Communication protocols
   - Guardian rights

3. **Pause Reasons** (reason_hash)
   - Detailed explanation
   - Supporting documentation
   - Expected duration

4. **Termination Outcomes** (outcome_hash)
   - Final assessment
   - Lessons learned
   - Settlement details
   - Dispute resolution (if any)

5. **Feedback Content** (feedback_hash)
   - Ratings and reviews
   - Improvement suggestions
   - Issue reports

### Storage Schema

```typescript
interface StoredDocument {
  hash: string;          // SHA-256 hash
  content: any;          // Encrypted JSON
  createdAt: number;
  accessPolicy: {
    viewers: Address[];  // Who can decrypt
    expiry?: number;
  };
  metadata: {
    type: DocumentType;
    referenceId: number; // engagement_id or feedback_id
  };
}
```

## Security Considerations

### Access Control

1. **Guardian Verification**: Must verify real-world relationships before allowing consent
2. **Session Recording**: Only actual participants can record sessions
3. **Pause Approval**: Enforce mutual approval policy when configured
4. **Admin Actions**: Safeguarding/policy terminations require admin auth

### Privacy Protection

1. **Minimal On-Chain Data**: Only hashes, amounts, status, addresses
2. **Encrypted Off-Chain**: Use end-to-end encryption for private documents
3. **Access Logs**: Audit who accesses sensitive documents
4. **Data Retention**: Implement appropriate retention policies

### Payment Security

1. **Escrow Protection**: Use escrow for all session payments
2. **Dispute Resolution**: Define clear dispute resolution process
3. **Partial Releases**: Support partial payments for partial completion
4. **Refund Logic**: Clear refund rules for termination scenarios

## Testing Integration

### Integration Test Scenarios

1. **Happy Path**: Create → Sessions → Complete → Certificate
2. **Guardian Path**: Create (pending) → Consent → Activate → Complete
3. **Pause Path**: Create → Pause → Approve → Resume → Complete
4. **Early Termination**: Create → Sessions → Terminate → Settle
5. **Consent Revocation**: Create → Consent → Revoke → Terminate
6. **Inactivity**: Create → Wait → Auto-terminate

## Deployment Checklist

- [ ] Deploy Mentorship contract
- [ ] Configure inactivity threshold
- [ ] Set pause approval policy
- [ ] Deploy off-chain API for document storage
- [ ] Set up event listeners
- [ ] Configure escrow integration
- [ ] Implement guardian verification workflow
- [ ] Set up certificate minting
- [ ] Configure feedback analysis pipeline
- [ ] Deploy monitoring/alerting
- [ ] Test end-to-end flows
- [ ] Security audit
- [ ] Load testing

## Migration from Existing System

If migrating from an existing mentorship system:

1. **Export existing engagements** with status mapping
2. **Generate hashes** for historical documents
3. **Batch create** engagements in contract
4. **Verify payment records** and reconcile
5. **Update frontend** to use new contract
6. **Run parallel** for transition period
7. **Deprecate old system** after verification

## Support & Maintenance

- Monitor event logs for anomalies
- Track feedback for platform improvements
- Regular security reviews
- Update consent templates as regulations change
- Performance optimization based on usage patterns
