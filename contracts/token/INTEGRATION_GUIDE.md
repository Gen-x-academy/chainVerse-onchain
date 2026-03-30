# Academy Vesting Contract - Integration Guide

## 📋 Overview

This guide demonstrates how to integrate the Academy Vesting Contract into your backend and frontend systems for managing academy rewards with secure claim flows.

---

## 🎯 Integration Architecture

```
Backend Service
├─ Grant vesting when awarding badges
├─ Track grant IDs in user profile
└─ Emit events for audit

Stellar Blockchain
├─ Store vesting schedules
├─ Enforce time-based unlocking
├─ Manage claims atomically
└─ Emit on-chain events

Off-Chain Indexer
├─ Subscribe to Grant events
├─ Subscribe to Claim events
├─ Subscribe to Revoke events
└─ Build user vesting history

User Interface
├─ Display vesting progress
├─ Show claim eligibility
├─ Execute claims
└─ Track claim history
```

---

## 🔧 Backend Integration

### 1. Initialize Contract (One-Time Setup)

```javascript
// backend/academy-vesting.service.ts

import { SorobanRpc, Networks, TransactionBuilder, Address } from '@stellar/js-sdk';

export class AcademyVestingService {
  private client: SorobanRpc.Server;
  private contractId: string;
  
  constructor(contractId: string) {
    this.client = new SorobanRpc.Server('https://soroban-testnet.stellar.org');
    this.contractId = contractId;
  }

  async initializeContract(
    adminAddress: string,
    tokenContractId: string,
    governanceContractId: string
  ) {
    // Call init() function
    const operation = {
      contractId: this.contractId,
      method: 'init',
      args: [
        Address.fromString(adminAddress),
        Address.fromString(tokenContractId),
        Address.fromString(governanceContractId),
      ],
    };
    
    console.log('✅ Contract initialized');
  }
}
```

### 2. Grant Vesting Schedule

```javascript
// backend/academy.service.ts

async grantBadgeRewards(
  userId: string,
  badgeType: string,
  tokenAmount: number
) {
  const user = await this.userService.findById(userId);
  
  // Calculate vesting parameters
  const now = Math.floor(Date.now() / 1000);
  const cliff = 86400;        // 1 day
  const duration = 31536000;  // 1 year
  
  // Grant on-chain
  const grantId = await this.vestingService.grantVesting(
    user.walletAddress,
    tokenAmount,
    now,
    cliff,
    duration
  );
  
  // Store grant in database
  await this.vestingRepository.create({
    userId,
    grantId,
    badgeType,
    amount: tokenAmount,
    startTime: now,
    cliff,
    duration,
    status: 'granted',
    grantedAt: new Date(),
  });
  
  // Emit event
  await this.eventService.emit('badge.awarded', {
    userId,
    grantId,
    badgeType,
    amount: tokenAmount,
  });
  
  console.log(`✅ Vesting grant created: ${grantId}`);
  return { grantId };
}
```

### 3. Monitor Grant Status

```javascript
async checkVestingStatus(grantId: number) {
  // Get schedule
  const schedule = await this.vestingService.getVesting(grantId);
  
  // Calculate vested amount
  const vested = await this.vestingService.getVestedAmount(grantId);
  
  const now = Math.floor(Date.now() / 1000);
  const percentVested = (vested / schedule.amount) * 100;
  
  console.log(`
    Grant ID: ${grantId}
    Total: ${schedule.amount} tokens
    Vested: ${vested} tokens (${percentVested}%)
    Claimed: ${schedule.claimed}
    Revoked: ${schedule.revoked}
  `);
  
  // Store in database
  await this.vestingStatusRepository.create({
    grantId,
    vested,
    percentVested,
    checkedAt: new Date(),
  });
  
  return {
    grantId,
    total: schedule.amount,
    vested,
    percentVested,
    claimed: schedule.claimed,
    revoked: schedule.revoked,
  };
}
```

### 4. Handle Revocation

```javascript
async revokeGrant(grantId: number, reason: string) {
  const adminAddress = process.env.ADMIN_ADDRESS;
  
  try {
    await this.vestingService.revoke(
      grantId,
      adminAddress,
      3600 * 24  // 1-day minimum revocation delay
    );
    
    console.log(`✅ Grant ${grantId} revoked: ${reason}`);
    
    // Update database
    await this.vestingRepository.update(grantId, {
      status: 'revoked',
      revokedAt: new Date(),
      revocationReason: reason,
    });
    
  } catch (error) {
    console.error(`❌ Revocation failed: ${error.message}`);
  }
}
```

---

## 🎨 Frontend Integration

### 1. Display Vesting Progress

```jsx
// frontend/components/VestingProgress.tsx

import React, { useState, useEffect } from 'react';
import { vestingService } from '../services/vesting.service';

export const VestingProgress: React.FC<{ grantId: number }> = ({ grantId }) => {
  const [vesting, setVesting] = useState<VestingStatus | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    const loadVestingStatus = async () => {
      try {
        const schedule = await vestingService.getVesting(grantId);
        const vested = await vestingService.getVestedAmount(grantId);
        
        setVesting({
          grantId,
          total: schedule.amount,
          vested,
          percentVested: (vested / schedule.amount) * 100,
          claimed: schedule.claimed,
          startTime: schedule.start_time,
          cliff: schedule.cliff,
          duration: schedule.duration,
        });
      } catch (error) {
        console.error('Failed to load vesting status:', error);
      } finally {
        setLoading(false);
      }
    };

    loadVestingStatus();
    const interval = setInterval(loadVestingStatus, 60000); // Refresh every minute
    return () => clearInterval(interval);
  }, [grantId]);

  if (loading) return <div>Loading...</div>;
  if (!vesting) return <div>No vesting found</div>;

  return (
    <div className="vesting-card">
      <h3>Vesting Progress</h3>
      
      <div className="progress-bar">
        <div 
          className="progress-fill"
          style={{ width: `${vesting.percentVested}%` }}
        />
      </div>
      
      <div className="vesting-details">
        <div>
          <strong>Total:</strong> {vesting.total} tokens
        </div>
        <div>
          <strong>Vested:</strong> {vesting.vested} tokens 
          ({vesting.percentVested.toFixed(1)}%)
        </div>
        <div>
          <strong>Status:</strong> {vesting.claimed ? '✅ Claimed' : '⏳ Pending'}
        </div>
      </div>

      <button 
        onClick={() => handleClaim(grantId)}
        disabled={vesting.claimed || vesting.percentVested < 100}
        className="claim-button"
      >
        {vesting.claimed ? 'Already Claimed' : 'Claim Tokens'}
      </button>
    </div>
  );
};
```

### 2. Claim Flow

```jsx
// frontend/components/ClaimFlow.tsx

export const ClaimFlow: React.FC<{ grantId: number }> = ({ grantId }) => {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);

  const handleClaim = async () => {
    setLoading(true);
    setError(null);
    
    try {
      // Get user's wallet
      const wallet = await window.freighter.getUserInfo();
      
      // Execute claim
      const claimedAmount = await vestingService.claim(
        grantId,
        wallet.publicKey
      );
      
      setSuccess(true);
      console.log(`✅ Successfully claimed ${claimedAmount} tokens`);
      
      // Update UI
      toast.success(`Claimed ${claimedAmount} tokens!`);
      
    } catch (err: any) {
      const errorCode = err.code;
      
      if (errorCode === 4002) {
        setError('⏳ Tokens not yet vested. Come back later!');
      } else if (errorCode === 4003) {
        setError('❌ Already claimed. Cannot claim twice!');
      } else if (errorCode === 4005) {
        setError('⚠️ Insufficient contract balance');
      } else if (errorCode === 4007) {
        setError('🚫 Grant has been revoked');
      } else {
        setError(`Error: ${err.message}`);
      }
      
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="claim-flow">
      <button onClick={handleClaim} disabled={loading}>
        {loading ? 'Processing...' : 'Claim Tokens'}
      </button>
      {error && <div className="error">{error}</div>}
      {success && <div className="success">Claim successful! 🎉</div>}
    </div>
  );
};
```

---

## 📊 Off-Chain Indexing

### 1. Subscribe to Events

```javascript
// indexer/vesting-indexer.ts

import { SorobanRpc } from '@stellar/js-sdk';

export class VestingIndexer {
  private client: SorobanRpc.Server;
  private db: Database;

  constructor() {
    this.client = new SorobanRpc.Server('https://soroban-testnet.stellar.org');
    this.db = new Database();
  }

  async subscribeToEvents() {
    // Get latest ledger
    let lastChecked = await this.db.getLastCheckedLedger() || 0;

    setInterval(async () => {
      try {
        // Get events since last check
        const events = await this.client.getEvents({
          startLedger: lastChecked,
          limit: 200,
        });

        for (const event of events.records) {
          if (event.contractId === VESTING_CONTRACT_ID) {
            await this.processEvent(event);
          }
        }

        lastChecked = events.records[events.records.length - 1].ledger;
        await this.db.updateLastCheckedLedger(lastChecked);

      } catch (error) {
        console.error('Indexing error:', error);
      }
    }, 5000); // Check every 5 seconds
  }

  private async processEvent(event: any) {
    const eventType = event.topic[0];

    if (eventType === 'grant') {
      await this.handleGrantEvent(event.value);
    } else if (eventType === 'claim') {
      await this.handleClaimEvent(event.value);
    } else if (eventType === 'revoke') {
      await this.handleRevokeEvent(event.value);
    }
  }

  private async handleGrantEvent(data: any) {
    console.log('✅ Grant Event:', {
      grantId: data.grant_id,
      beneficiary: data.beneficiary,
      amount: data.amount,
      cliff: data.cliff,
      duration: data.duration,
    });

    await this.db.saveGrant({
      grantId: data.grant_id,
      beneficiary: data.beneficiary,
      amount: data.amount,
      startTime: data.start_time,
      cliff: data.cliff,
      duration: data.duration,
      grantedAt: new Date(),
      grantedBy: data.granted_by,
    });
  }

  private async handleClaimEvent(data: any) {
    console.log('✅ Claim Event:', {
      grantId: data.grant_id,
      beneficiary: data.beneficiary,
      amount: data.amount,
    });

    await this.db.updateGrant(data.grant_id, {
      claimed: true,
      claimedAt: new Date(),
      claimedAmount: data.amount,
    });
  }

  private async handleRevokeEvent(data: any) {
    console.log('✅ Revoke Event:', {
      grantId: data.grant_id,
      revokedBy: data.revoked_by,
    });

    await this.db.updateGrant(data.grant_id, {
      revoked: true,
      revokedAt: new Date(),
      revokedBy: data.revoked_by,
    });
  }
}
```

### 2. Query Indexed Data

```javascript
// api/vesting.api.ts

export class VestingAPI {
  async getUserVesting(userId: string) {
    // Get all grants for user's wallet
    const grants = await db.query(
      'SELECT * FROM vesting_grants WHERE beneficiary = $1 ORDER BY grantedAt DESC',
      [userWallet]
    );

    return grants.map(grant => ({
      grantId: grant.grant_id,
      amount: grant.amount,
      vested: calculateVested(grant),
      claimed: grant.claimed,
      revoked: grant.revoked,
      vestingProgress: {
        start: grant.start_time,
        cliff: grant.cliff,
        duration: grant.duration,
        percentComplete: calculatePercentVested(grant),
      },
    }));
  }

  async getVestingHistory(grantId: number) {
    return await db.query(
      'SELECT * FROM vesting_events WHERE grant_id = $1 ORDER BY timestamp',
      [grantId]
    );
  }

  async getAdminStats() {
    return {
      totalGranted: await db.count('vesting_grants'),
      totalClaimed: await db.count('vesting_grants', { claimed: true }),
      totalRevoked: await db.count('vesting_grants', { revoked: true }),
      pending: await db.count('vesting_grants', { claimed: false, revoked: false }),
    };
  }
}
```

---

## 🧪 Integration Testing

### End-to-End Test

```javascript
// tests/vesting-integration.test.ts

describe('Vesting Integration', () => {
  let vestingService: VestingService;
  let testUser: User;
  let grantId: number;

  beforeAll(async () => {
    vestingService = new VestingService(TESTNET_CONTRACT_ID);
    testUser = await createTestUser();
  });

  test('Should grant vesting schedule', async () => {
    grantId = await vestingService.grantVesting(
      testUser.walletAddress,
      1000,           // amount
      Math.floor(Date.now() / 1000),
      100,            // cliff (seconds)
      3600            // duration (seconds)
    );

    expect(grantId).toBeGreaterThan(0);
  });

  test('Should not allow claim before cliff', async () => {
    const error = await vestingService.claim(grantId, testUser.walletAddress);
    expect(error.code).toBe(4002); // NotVested
  });

  test('Should allow claim after cliff', async () => {
    // Fast-forward time
    await fastForward(150);
    
    const claimed = await vestingService.claim(grantId, testUser.walletAddress);
    expect(claimed).toBeGreaterThan(0);
  });

  test('Should prevent double-claim', async () => {
    const error = await vestingService.claim(grantId, testUser.walletAddress);
    expect(error.code).toBe(4003); // AlreadyClaimed
  });

  test('Should allow revocation with timelock', async () => {
    const grantId2 = await vestingService.grantVesting(
      testUser.walletAddress,
      500,
      Math.floor(Date.now() / 1000),
      100,
      3600
    );

    // Try to revoke too early
    const error1 = await vestingService.revoke(grantId2, ADMIN, 3600);
    expect(error1.code).toBe(4009); // NotEnoughTimeForRevoke

    // Wait for timelock
    await fastForward(3600);
    
    const success = await vestingService.revoke(grantId2, ADMIN, 3600);
    expect(success).toBe(true);
  });

  test('Should emit events for indexing', async () => {
    const events: any[] = [];
    
    vestingService.on('grant', (event) => events.push(event));
    vestingService.on('claim', (event) => events.push(event));
    vestingService.on('revoke', (event) => events.push(event));

    // Perform actions
    await vestingService.grantVesting(...params);
    
    // Verify events
    expect(events.length).toBeGreaterThan(0);
    expect(events[0].topic).toBe('grant');
  });
});
```

---

## 📈 Monitoring

### Health Check

```javascript
async function monitorVestingHealth() {
  const stats = {
    totalGrants: await db.count('vesting_grants'),
    totalClaimed: await db.count('vesting_grants', { claimed: true }),
    totalRevoked: await db.count('vesting_grants', { revoked: true }),
    failedClaims: await db.count('claim_errors', { recent: true }),
    averageVestingTime: await db.query('SELECT AVG(duration) FROM vesting_grants'),
  };

  console.log('📊 Vesting Health:', stats);

  // Alert on anomalies
  if (stats.failedClaims > 10) {
    await alertService.send('HIGH_FAILED_CLAIMS', stats);
  }
}
```

---

## 🚀 Deployment Checklist

### Pre-Deployment
- [ ] All tests passing (`cargo test`)
- [ ] Contract compiled successfully
- [ ] Token contract deployed
- [ ] Admin address configured
- [ ] Governance address configured (if needed)
- [ ] Testnet funding ready

### Deployment
- [ ] Deploy to testnet
- [ ] Initialize contract
- [ ] Setup indexer
- [ ] Configure backend service
- [ ] Test grant flow end-to-end
- [ ] Monitor events

### Post-Deployment
- [ ] Enable frontend vesting UI
- [ ] Start off-chain indexing
- [ ] Setup monitoring/alerts
- [ ] Document for ops team
- [ ] Plan mainnet migration

---

## 📚 Related Documentation

- [Vesting Design](./VESTING_DESIGN.md)
- [Quick Reference](./VESTING_QUICK_REFERENCE.md)
- [Code](./src/vesting.rs)
- [Tests](./src/test.rs)

