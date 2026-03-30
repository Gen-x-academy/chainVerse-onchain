use soroban_sdk::{Env, testutils::Address as _};
use token::TokenContract;

#[test]
#[should_panic(expected = "Unauthorized")]
fn non_admin_cannot_mint() {
    let env = Env::default();

    let admin = Address::random(&env);
    let attacker = Address::random(&env);

    TokenContract::initialize(env.clone(), admin);

    // attacker tries mint
    TokenContract::mint(env, attacker, 100);
}
