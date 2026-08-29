use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    ContractPaused = 4,
    CertificateExists = 5,
    InvalidProof = 6,
    SoulboundTransferNotAllowed = 7,
    CertificateNotFound = 8,
    ProofExpired = 9,
    NonceAlreadyConsumed = 10,
    // Fix #841: admin transfer errors
    NoPendingTransfer = 11,
    PendingAdminExpired = 12,
    NotPendingAdmin = 13,
    // Fix #834: backend public key must be exactly 32 bytes (Ed25519)
    InvalidPublicKey = 14,
    // Fix #835: signing key rotation errors
    NoPendingKeyRotation = 15,
    PendingKeyRotationExpired = 16,
    // Fix #836: minter rotation errors
    NoPendingMinterRotation = 17,
    PendingMinterRotationExpired = 18,
}
