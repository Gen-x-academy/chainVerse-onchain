/// Fix #833 — Domain-separated Ed25519 signature verification.
///
/// The canonical message has the structure:
///
/// ```text
/// "CHAINVERSE_CERT:" | contract_address (XDR) | network_id (32 bytes) | recipient (XDR)
///                    | course_id (32 bytes)    | nonce (32 bytes)      | expires_at (8 bytes BE)
/// ```
///
/// Binding the message to the contract address and network ID prevents
/// cross-contract and cross-environment (mainnet/testnet) replay attacks.
use ed25519_dalek::{Signature, VerifyingKey};
use soroban_sdk::{xdr::ToXdr, Address, Bytes, BytesN, Env};

use crate::ContractError;

/// Domain prefix — must match the backend signing code exactly.
const DOMAIN_PREFIX: &[u8] = b"CHAINVERSE_CERT:";

/// Verifies an Ed25519 backend proof using domain-separated message construction.
///
/// The message committed to by the signature is:
/// `DOMAIN_PREFIX || contract_id_xdr || network_id || recipient_xdr || course_id || nonce || expires_at_be`
pub fn verify_backend_proof(
    env: &Env,
    backend_public_key: &BytesN<32>,
    recipient: &Address,
    course_id: &BytesN<32>,
    nonce: &BytesN<32>,
    expires_at: u64,
    proof: &Bytes,
) -> Result<(), ContractError> {
    // --- decode signature ---
    if proof.len() != 64 {
        return Err(ContractError::InvalidProof);
    }
    let mut sig_bytes = [0u8; 64];
    proof.copy_into_slice(&mut sig_bytes);
    let signature = Signature::from_bytes(&sig_bytes);

    // --- build verifying key ---
    let pk_arr: [u8; 32] = backend_public_key.into();
    let verifying_key =
        VerifyingKey::from_bytes(&pk_arr).map_err(|_| ContractError::InvalidProof)?;

    // --- construct domain-separated message ---
    // Fix #833: include contract address and network ID so a proof generated for
    // contract A on testnet cannot be replayed on contract B or on mainnet.
    let contract_id = env.current_contract_address();
    let network_id = env.ledger().network_id();

    let mut msg = Bytes::new(env);
    // 1. Domain prefix
    msg.append(&Bytes::from_slice(env, DOMAIN_PREFIX));
    // 2. Contract address (serialised to XDR for a stable canonical form)
    msg.append(&contract_id.to_xdr(env));
    // 3. Network passphrase hash (32 bytes) — distinguishes mainnet from testnet
    msg.append(&network_id.into());
    // 4. Recipient address
    msg.append(&recipient.to_xdr(env));
    // 5. Course ID (32 bytes)
    msg.append(&course_id.clone().into());
    // 6. Nonce (32 bytes) — prevents replay within the same environment
    msg.append(&nonce.clone().into());
    // 7. Expiry timestamp — big-endian u64
    msg.append(&Bytes::from_slice(env, &expires_at.to_be_bytes()));

    // --- copy to stack slice for dalek ---
    let msg_len = msg.len() as usize;
    let mut buf = [0u8; 512];
    if msg_len > buf.len() {
        return Err(ContractError::InvalidProof);
    }
    msg.copy_into_slice(&mut buf[..msg_len]);

    verifying_key
        .verify_strict(&buf[..msg_len], &signature)
        .map_err(|_| ContractError::InvalidProof)
}
