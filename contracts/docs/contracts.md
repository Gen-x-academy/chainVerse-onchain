### Smart Contract Error & Event Standards

Shared Error Codes

# All contracts must use the shared ContractError enum.

| Code | Name              | Description                              |
| ---- | ----------------- | ---------------------------------------- |
| 1    | Unauthorized      | Caller does not have required permission |
| 2    | AlreadyPurchased  | Course already purchased                 |
| 3    | InvalidPayment    | Payment amount invalid or incorrect      |
| 4    | AlreadyRewarded   | Reward already claimed                   |
| 5    | CertificateExists | Certificate already minted               |
| 6    | ContractPaused    | Contract currently paused                |

Errors must be imported from the shared crate:

```
use shared::ContractError;
```

### Event Naming Convention

All events follow:

```
chainverse:{domain}:{event_name}
```

Rules:

- All lowercase

- Snake_case

- 3-level topic structure

- Consistent across all contracts

Standard Events
CoursePurchased

## Topic:

chainverse:course:purchased

# Payload:

```
(buyer: Address, course_id: u64, amount: i128)
```

RewardClaimed

## Topic:

chainverse:reward:claimed

# Payload:

```
(user: Address, reward_id: u64, amount: i128)
```

CertificateMinted

## Topic:

chainverse:certificate:minted

# Payload:

```
(user: Address, course_id: u64, token_id: u64)
```
