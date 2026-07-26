# Reward Contract

## Treasury Allowance Requirement

Before calling `claim_reward`, the treasury address **must pre-approve** the reward contract as a spender for the reward token. If the allowance is zero or less than the reward amount, claims will fail with an `InsufficientTreasuryAllowance` error.

### How to Approve

The treasury should call the token contract's `approve` method, setting the spender to the reward contract address and the amount to at least the total rewards to be distributed.

**Example:**

```
token_client.approve(&treasury, &reward_contract_address, &amount);
```

- `treasury`: The address holding the reward tokens
- `reward_contract_address`: The deployed reward contract address
- `amount`: The total allowance to grant

**Note:** If the allowance is not set or is insufficient, reward claims will revert.
