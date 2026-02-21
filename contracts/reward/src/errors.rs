use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Error {
    Unauthorized = 1,
    InvalidSignature = 2,
}

use soroban_sdk::contracttype;

#[derive(Clone)]
#[contracttype]
pub enum RewardError {
    SignatureExpired = 1,
    NonceAlreadyUsed = 2,
    InvalidSignature = 3,
}