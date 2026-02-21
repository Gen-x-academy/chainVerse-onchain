#![no_std]

use soroban_sdk::{
    contract, contractimpl, Address, Env, BytesN, symbol_short,
};
use storage::DataKey;
use types::Certificate;

mod types;
mod contract;
mod storage;
mod events;
mod errors;

pub use contract::*;

#[contract]
pub struct CertificateContract;

#[contractimpl]
impl CertificateContract {

    // ============================
    // Generate deterministic certificate ID
    // ============================
    fn generate_certificate_id(
        env: &Env,
        wallet: &Address,
        course_id: u32,
    ) -> BytesN<32> {
        let hash_input = (wallet.clone(), course_id);
        env.crypto().sha256(&env.serialize(&hash_input).unwrap())
    }

    // ============================
    // Mint Certificate (internal)
    // ============================
    pub fn issue_certificate(
        env: Env,
        wallet: Address,
        course_id: u32,
        metadata_hash: BytesN<32>,
    ) -> BytesN<32> {

        wallet.require_auth();

        // Prevent duplicate certificate
        let existing = env.storage().instance().get::<_, BytesN<32>>(
            &DataKey::WalletCourse(wallet.clone(), course_id),
        );

        if existing.is_some() {
            panic!("CertificateExists");
        }

        let certificate_id =
            Self::generate_certificate_id(&env, &wallet, course_id);

        let certificate = Certificate {
            wallet: wallet.clone(),
            course_id,
            metadata_hash,
            timestamp: env.ledger().timestamp(),
        };

        // Store certificate data
        env.storage()
            .instance()
            .set(&DataKey::Certificate(certificate_id.clone()), &certificate);

        // Map wallet + course → certificate_id
        env.storage().instance().set(
            &DataKey::WalletCourse(wallet.clone(), course_id),
            &certificate_id,
        );

        certificate_id
    }

    // ============================
    // Query Certificate
    // ============================
    pub fn get_certificate(
        env: Env,
        certificate_id: BytesN<32>,
    ) -> Certificate {
        env.storage()
            .instance()
            .get(&DataKey::Certificate(certificate_id))
            .expect("CertificateNotFound")
    }

    // ============================
    // Query by Wallet + Course
    // ============================
    pub fn get_certificate_by_wallet(
        env: Env,
        wallet: Address,
        course_id: u32,
    ) -> Certificate {

        let cert_id: BytesN<32> = env
            .storage()
            .instance()
            .get(&DataKey::WalletCourse(wallet, course_id))
            .expect("CertificateNotFound");

        env.storage()
            .instance()
            .get(&DataKey::Certificate(cert_id))
            .unwrap()
    }
}