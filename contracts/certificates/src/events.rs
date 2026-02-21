use soroban_sdk::{Address, Env, symbol_short};

pub fn certificate_minted(e: &Env, wallet: Address, cert_id: u64) {
    e.events().publish(
        (symbol_short!("CertificateMinted"),),
        (wallet, cert_id),
    );
}