//! Integration tests for the escrow lifecycle.
//!
//! This test verifies the full escrow lifecycle:
//! 1. Create an escrow and confirm tokens are moved to the contract.
//! 2. Release the escrow and confirm tokens are transferred to the recipient.
//! 3. Create a second escrow, let it expire, and confirm the refund returns tokens to the depositor.
//!
//! The test uses the `soroban-sdk` testutils feature to simulate the Soroban environment.
//! Adjust the contract calls and parameters according to the actual escrow contract API.

#[cfg(test)]
mod tests {
    use soroban_sdk::{testutils::ed25519::generate, testutils::Env as TestEnv, Env, Symbol, Vec};
    use soroban_sdk::token::Client as TokenClient;
    // Import the escrow contract interface (replace with actual path if different).
    use crate::escrow::{EscrowContract, EscrowClient};

    #[test]
    fn escrow_lifecycle() {
        // Initialize test environment.
        let env = TestEnv::default();
        // Setup participants.
        let depositor = generate(&env);
        let recipient = generate(&env);
        let token_admin = generate(&env);

        // Deploy a mock token contract.
        let token_id = env.register_contract(None, token_admin.clone());
        let token = TokenClient::new(&env, &token_id);
        // Issue some tokens to depositor.
        token.mint(&depositor, &1000);

        // Deploy the escrow contract.
        let escrow_id = env.register_contract(None, EscrowContract {});
        let escrow = EscrowClient::new(&env, &escrow_id);

        // 1. Create escrow.
        escrow.create(&depositor, &recipient, &token_id, &500, &100, &env.ledger().timestamp() + 5000);
        // Verify depositor balance decreased.
        assert_eq!(token.balance(&depositor), 500);
        assert_eq!(token.balance(&escrow_id), 500);

        // 2. Release escrow.
        escrow.release(&depositor);
        // Verify recipient received tokens.
        assert_eq!(token.balance(&recipient), 500);
        assert_eq!(token.balance(&escrow_id), 0);

        // 3. Create second escrow that will expire.
        escrow.create(&depositor, &recipient, &token_id, &200, &50, &env.ledger().timestamp() + 10);
        // Fast forward ledger time beyond expiration.
        env.ledger().set_timestamp(env.ledger().timestamp() + 20);
        // Attempt refund.
        escrow.refund(&depositor);
        // Verify tokens returned to depositor.
        assert_eq!(token.balance(&depositor), 800);
        assert_eq!(token.balance(&escrow_id), 0);
    }
}
