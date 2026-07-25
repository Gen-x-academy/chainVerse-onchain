// Allow deprecated events API until migration to #[contractevent] macro
#![allow(deprecated)]

use crate::errors::Error;
use crate::membership_token::DataKey;
use crate::types::{RoyaltyConfig, RoyaltyInfo, RoyaltyRecipient};
use soroban_sdk::{symbol_short, token, Address, BytesN, Env, Vec};

pub struct RoyaltyModule;

impl RoyaltyModule {
    /// Maximum allowed total royalty percentage (basis points: 10000 = 100%)
    const MAX_ROYALTY_BPS: u32 = 10000;

    /// Validates royalty configuration
    fn validate_config(recipients: &Vec<RoyaltyRecipient>) -> Result<u32, Error> {
        let mut total_percentage: u32 = 0;

        for recipient in recipients.iter() {
            total_percentage = total_percentage.saturating_add(recipient.percentage);
        }

        if total_percentage > Self::MAX_ROYALTY_BPS {
            return Err(Error::InvalidPaymentAmount);
        }

        Ok(total_percentage)
    }

    /// Sets or updates the royalty configuration for a token
    pub fn set_royalty(
        env: Env,
        admin: Address,
        token_id: BytesN<32>,
        recipients: Vec<RoyaltyRecipient>,
    ) -> Result<(), Error> {
        // Require admin authorization
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::AdminNotSet)?;
        stored_admin.require_auth();
        if stored_admin != admin {
            return Err(Error::Unauthorized);
        }

        // Validation
        let _ = Self::validate_config(&recipients)?;

        let config = RoyaltyConfig {
            token_id: token_id.clone(),
            recipients: recipients.clone(),
            enabled: true,
        };

        // Emit set event
        env.events().publish(
            (symbol_short!("roy_set"), token_id.clone()),
            (recipients.len(), env.ledger().timestamp()),
        );

        const MIN_TTL: u32 = 100_000;
        const MAX_TTL: u32 = 200_000;

        env.storage()
            .persistent()
            .set(&DataKey::Royalty(token_id.clone()), &config);

        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Royalty(token_id), MIN_TTL, MAX_TTL);

        Ok(())
    }

    /// Calculates and transfers royalty payments for each recipient.
    pub fn calculate_and_pay_royalties(
        env: &Env,
        token_id: &BytesN<32>,
        payment_token: &Address,
        sale_price: i128,
        payer: &Address,
    ) -> Result<i128, Error> {
        if sale_price <= 0 {
            return Ok(0);
        }

        let config: Option<RoyaltyConfig> = env
            .storage()
            .persistent()
            .get(&DataKey::Royalty(token_id.clone()));

        if let Some(cfg) = config {
            if !cfg.enabled || cfg.recipients.is_empty() {
                return Ok(0);
            }

            let token_client = token::Client::new(env, payment_token);
            let mut total_royalty_amount: i128 = 0;

            for recipient in cfg.recipients.iter() {
                let amount =
                    (sale_price * recipient.percentage as i128) / Self::MAX_ROYALTY_BPS as i128;

                if amount > 0 {
                    total_royalty_amount += amount;

                    // Transfer royalty to recipient
                    token_client.transfer(payer, &recipient.address, &amount);

                    env.events().publish(
                        (
                            symbol_short!("roy_paid"),
                            token_id.clone(),
                            recipient.address.clone(),
                        ),
                        (payment_token.clone(), amount, env.ledger().timestamp()),
                    );
                }
            }

            return Ok(total_royalty_amount);
        }

        Ok(0)
    }

    /// Get details of royalty configuration
    pub fn get_royalty_info(env: Env, token_id: BytesN<32>) -> Option<RoyaltyInfo> {
        let config_opt: Option<RoyaltyConfig> =
            env.storage().persistent().get(&DataKey::Royalty(token_id));

        if let Some(config) = config_opt {
            let mut total_percentage = 0;
            for r in config.recipients.iter() {
                total_percentage += r.percentage;
            }

            Some(RoyaltyInfo {
                config,
                total_percentage,
            })
        } else {
            None
        }
    }
}
