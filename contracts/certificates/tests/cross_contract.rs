#![cfg(test)]

extern crate std;

use academy_rewards::AcademyRewardsContract;
use messaging::UpgradeableMessagingContract;
use shared::governance::ProposalStatus;
use social_rewards::SocialRewardsContract;
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short,
    testutils::{Address as _, Ledger},
    token, Address, Env, String, Vec,
};
use trading::UpgradeableTradingContract;

#[contract]
pub struct MockTokenContract;

#[contracttype]
#[derive(Clone)]
pub enum TokenDataKey {
    Balance(Address),
}

#[contractimpl]
impl MockTokenContract {
    pub fn mint(env: Env, to: Address, amount: i128) {
        let current = Self::balance(env.clone(), to.clone());
        let updated = current.checked_add(amount).expect("overflow");
        env.storage()
            .persistent()
            .set(&TokenDataKey::Balance(to), &updated);
    }

    pub fn balance(env: Env, id: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&TokenDataKey::Balance(id))
            .unwrap_or(0)
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();

        let from_balance = Self::balance(env.clone(), from.clone());
        if from_balance < amount {
            panic!("insufficient balance")
        }

        let to_balance = Self::balance(env.clone(), to.clone());

        env.storage()
            .persistent()
            .set(&TokenDataKey::Balance(from), &(from_balance - amount));
        env.storage()
            .persistent()
            .set(&TokenDataKey::Balance(to), &(to_balance + amount));
    }
}

#[test]
fn test_academy_rewards_trigger_social_rewards() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.timestamp = 1000);
    env.mock_all_auths();

    let academy_id = env.register_contract(None, AcademyRewardsContract);
    let academy = academy_rewards::AcademyRewardsContractClient::new(&env, &academy_id);

    let social_id = env.register_contract(None, SocialRewardsContract);
    let social = social_rewards::SocialRewardsContractClient::new(&env, &social_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    academy.initialize(&admin);
    social.init(&admin);

    academy.create_badge_type(
        &admin,
        &1u32,
        &String::from_str(&env, "Gold"),
        &500u32,
        &5u32,
        &0u64,
    );
    academy.mint_badge(&admin, &user, &1u32);

    let discount = academy.redeem_badge(&user, &String::from_str(&env, "tx-1"));
    assert_eq!(discount, 500);

    // Integration behavior: a successful badge redemption triggers social reward crediting.
    social.add_reward(&user, &(discount as i128));

    let record = academy.get_redemption_history(&user, &0u32).unwrap();
    assert_eq!(record.discount_applied, 500);
}

#[test]
fn test_trading_interacts_with_fee_distribution() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.timestamp = 1000);
    env.mock_all_auths();

    let token_id = env.register_contract(None, MockTokenContract);
    let token_admin = MockTokenContractClient::new(&env, &token_id);

    let trading_id = env.register_contract(None, UpgradeableTradingContract);
    let trading = trading::UpgradeableTradingContractClient::new(&env, &trading_id);

    let admin = Address::generate(&env);
    let approver = Address::generate(&env);
    let executor = Address::generate(&env);
    let trader = Address::generate(&env);
    let fee_recipient = Address::generate(&env);

    let mut approvers = Vec::new(&env);
    approvers.push_back(approver);

    trading.init(&admin, &approvers, &executor);

    token_admin.mint(&trader, &1000i128);

    let fee_before_trader = token::Client::new(&env, &token_id).balance(&trader);
    let fee_before_recipient = token::Client::new(&env, &token_id).balance(&fee_recipient);

    let trade_id = trading.trade(
        &trader,
        &symbol_short!("XLMUSD"),
        &250i128,
        &100i128,
        &true,
        &token_id,
        &25i128,
        &fee_recipient,
    );

    assert_eq!(trade_id, 1);

    let fee_after_trader = token::Client::new(&env, &token_id).balance(&trader);
    let fee_after_recipient = token::Client::new(&env, &token_id).balance(&fee_recipient);

    assert_eq!(fee_before_trader - fee_after_trader, 25);
    assert_eq!(fee_after_recipient - fee_before_recipient, 25);

    let stats = trading.get_stats();
    assert_eq!(stats.total_trades, 1);
    assert_eq!(stats.total_volume, 250);
}

#[test]
fn test_messaging_notifications_from_other_contract_flows() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.timestamp = 1000);
    env.mock_all_auths();

    let academy_id = env.register_contract(None, AcademyRewardsContract);
    let academy = academy_rewards::AcademyRewardsContractClient::new(&env, &academy_id);

    let messaging_id = env.register_contract(None, UpgradeableMessagingContract);
    let messaging = messaging::UpgradeableMessagingContractClient::new(&env, &messaging_id);

    let admin = Address::generate(&env);
    let approver = Address::generate(&env);
    let executor = Address::generate(&env);
    let notifier = Address::generate(&env);
    let user = Address::generate(&env);

    let mut approvers = Vec::new(&env);
    approvers.push_back(approver);

    messaging.init(&admin, &approvers, &executor);

    academy.initialize(&admin);
    academy.create_badge_type(
        &admin,
        &2u32,
        &String::from_str(&env, "Silver"),
        &250u32,
        &3u32,
        &0u64,
    );
    academy.mint_badge(&admin, &user, &2u32);

    let discount = academy.redeem_badge(&user, &String::from_str(&env, "tx-2"));
    let payload = String::from_str(&env, "Your academy badge was redeemed successfully");

    let message_id = messaging.send_message(&notifier, &user, &payload);
    assert_eq!(message_id, 1);
    assert_eq!(discount, 250);

    let unread = messaging.get_unread_count(&user);
    assert_eq!(unread, 1);

    let notifications = messaging.get_messages(&user, &false, &true, &true);
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications.get(0).unwrap().payload, payload);
}

#[test]
fn test_shared_governance_module_across_contracts() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.timestamp = 1000);
    env.mock_all_auths();

    let trading_id = env.register_contract(None, UpgradeableTradingContract);
    let trading = trading::UpgradeableTradingContractClient::new(&env, &trading_id);

    let messaging_id = env.register_contract(None, UpgradeableMessagingContract);
    let messaging = messaging::UpgradeableMessagingContractClient::new(&env, &messaging_id);

    let admin = Address::generate(&env);
    let approver = Address::generate(&env);
    let executor = Address::generate(&env);

    let mut approvers = Vec::new(&env);
    approvers.push_back(approver.clone());

    trading.init(&admin, &approvers, &executor);
    messaging.init(&admin, &approvers, &executor);

    let trading_proposal = trading.propose_upgrade(
        &admin,
        &symbol_short!("tv2hash"),
        &symbol_short!("UpgrTrade"),
        &approvers,
        &1u32,
        &3600u64,
    );
    trading.approve_upgrade(&trading_proposal, &approver);

    let messaging_proposal = messaging.propose_upgrade(
        &admin,
        &symbol_short!("mv2hash"),
        &symbol_short!("UpgrMsg"),
        &approvers,
        &1u32,
        &3600u64,
    );
    messaging.approve_upgrade(&messaging_proposal, &approver);

    let trade_status = trading.get_upgrade_proposal(&trading_proposal).status;
    let msg_status = messaging.get_upgrade_proposal(&messaging_proposal).status;

    assert_eq!(trade_status, ProposalStatus::Approved);
    assert_eq!(msg_status, ProposalStatus::Approved);
}
