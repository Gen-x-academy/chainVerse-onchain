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
    // Fix #841: no pending admin transfer has been proposed.
    NoPendingTransfer = 9,
    // Fix #841: the pending admin transfer has expired and can no longer be accepted.
    PendingAdminExpired = 10,
    // Fix #841: caller is not the nominated pending admin.
    NotPendingAdmin = 11,
}
