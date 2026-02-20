#![no_std]

use soroban_sdk::{contracterror};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    Unauthorized = 1,
    AlreadyPurchased = 2,
    InvalidPayment = 3,
    AlreadyRewarded = 4,
    CertificateExists = 5,
    ContractPaused = 6,
}