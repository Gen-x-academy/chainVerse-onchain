#![no_std]

//! Scholarship/bursary award disbursement contract.
//!
//! Scope of this pass (issues #1096, #1097): turn an eligible award
//! installment into exactly one *stable, immutable* payment intent before
//! any external execution, and require a recipient to prove control of
//! their configured payout wallet before that wallet can receive its
//! first payment (or have an existing one changed).
//!
//! - **#1096 — Create idempotent disbursement intents.**
//!   `create_intent()` derives a deterministic intent key from
//!   `(program_id, recipient, installment)` and stores the intent under
//!   it. Calling it again with identical parameters returns the *same*
//!   intent without changing anything (retries reconcile existing state);
//!   calling it with different `amount`/`wallet` under the same key is
//!   rejected with `IntentConflict`. A recipient and amount are therefore
//!   immutable once an intent exists — there is no update path, and
//!   `record_execution()` can only move a `Pending` intent to `Executed`
//!   once, so the same installment can never be paid twice.
//! - **#1097 — Validate payout wallet ownership.** A recipient proves
//!   control of their configured wallet by opening a challenge
//!   (`open_wallet_challenge`) and returning an ed25519 signature from the
//!   wallet over the contract-derived, **domain-separated** payload
//!   (`wallet_challenge_payload`), verified by
//!   `confirm_wallet_challenge`. Challenges expire and are single-use.
//!   Changing the configured wallet bumps a per-recipient `epoch` and
//!   clears verification, which structurally places every intent created
//!   under the old wallet on safe hold until a new intent exists for the
//!   newly-verified wallet.
//!
//! On-chain state never contains evidence, documents, or bank detail: only
//! addresses, a per-wallet ed25519 public key, integer amounts/installment
//! indices, status enums, and timestamps. See
//! `contracts/docs/scholarship-disbursements.md` for ownership, privacy,
//! migration, and operational notes.

use ed25519_dalek::{Signature, VerifyingKey};
use shared::signing::{self, MessageType, SigningDomain, ENVELOPE_VERSION};
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, xdr::ToXdr, Address, Bytes,
    BytesN, Env,
};

const CONTRACT_VERSION: u32 = 1;

// TTL constants: ~1 year at 6-second ledgers, matching the sibling scholarship contracts.
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

/// Upper bound on a challenge's lifetime. Bounded so a caller cannot create
/// a challenge that is effectively permanent.
const MAX_CHALLENGE_TTL_SECONDS: u64 = 86_400;

/// #1096 — domain tag folded into the derived intent key so it can never
/// collide with a hash produced elsewhere.
const INTENT_DOMAIN_TAG: &[u8] = b"ChainVerse.Scholarship.DisbursementIntent";

/// #1097 — domain tag folded into the signed challenge body. The
/// surrounding [`SigningDomain`] additionally commits the network, contract,
/// and message type, so a proof for this contract cannot be replayed
/// against another.
const WALLET_PROOF_DOMAIN_TAG: &[u8] = b"ChainVerse.Scholarship.PayoutWallet";

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    /// #1096 — caller is neither the admin nor an allowlisted creator.
    NotAuthorizedCreator = 4,
    /// #1096 — an installment amount must be strictly positive.
    InvalidAmount = 5,
    /// #1097 — the recipient has no configured payout wallet.
    WalletNotFound = 6,
    /// #1097 — no ed25519 key has been registered for the wallet.
    WalletKeyNotFound = 7,
    ChallengeNotFound = 8,
    /// #1097 — the challenge is past its expiry.
    ChallengeExpired = 9,
    /// #1097 — the challenge has already been confirmed.
    ChallengeAlreadyUsed = 10,
    /// #1097 — the challenge does not match the recipient's configured wallet,
    /// or the wallet changed after the challenge was opened.
    ChallengeWalletMismatch = 11,
    /// #1097 — the ed25519 proof did not verify against the wallet's key.
    InvalidSignature = 12,
    IntentNotFound = 13,
    /// #1096 — an intent already exists for this key with different immutable fields.
    IntentConflict = 14,
    /// #1096 — the intent was already recorded as executed.
    AlreadyExecuted = 15,
    /// #1096 — the intent is on safe hold (wallet unverified or superseded).
    IntentOnHold = 16,
    /// #1097 — the wallet being configured/challenged is not the expected one.
    WalletMismatch = 17,
    /// #1097 — the presented public key is not the one registered for the wallet.
    WalletKeyMismatch = 18,
    /// #1097 — a challenge TTL of zero or above `MAX_CHALLENGE_TTL_SECONDS`.
    InvalidChallengeTtl = 19,
    /// #1097 — a challenge counter or expiry overflowed.
    Overflow = 20,
    /// #1096 — the intent was already cancelled.
    AlreadyCancelled = 21,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// #1097 — the Stellar network id (`sha256(passphrase)`) the proof is bound to.
    NetworkId,
    /// #1096 — address allowlisted to create/execute disbursement intents.
    Creator(Address),
    /// #1097 — ed25519 public key registered for a wallet address.
    WalletKey(Address),
    /// #1097 — the payout wallet configured for a recipient.
    WalletBinding(Address),
    /// #1097 — monotonic challenge sequence.
    ChallengeCounter,
    /// #1097 — a single wallet-ownership challenge.
    Challenge(u64),
    /// #1096 — a disbursement intent, keyed by its derived id.
    Intent(BytesN<32>),
}

/// #1097 — a recipient's configured payout wallet and whether control of
/// it has been proven. `epoch` increments on every wallet change, so intents
/// can record which wallet-generation they target without any list to walk.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PayoutWallet {
    pub wallet: Address,
    pub verified: bool,
    pub epoch: u32,
    pub updated_at: u64,
}

/// #1097 — a one-time, expiring challenge proving control of `wallet`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WalletChallenge {
    pub id: u64,
    pub recipient: Address,
    pub wallet: Address,
    pub expires_at: u64,
    pub used: bool,
    pub created_at: u64,
}

/// #1096 — an intent's terminal-aware status.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntentStatus {
    Pending,
    Executed,
    Cancelled,
}

/// #1096 — one stable payment intent for an award installment. `recipient`,
/// `wallet`, `amount`, and `installment` are set once and never mutated.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisbursementIntent {
    pub id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub recipient: Address,
    /// Payout target captured at creation; immutable for the intent's life.
    pub wallet: Address,
    /// Immutable installment amount.
    pub amount: i128,
    /// Immutable installment index within the award.
    pub installment: u32,
    /// The recipient's wallet-generation this intent targets (#1097).
    pub wallet_epoch: u32,
    pub created_at: u64,
    pub created_by: Address,
    pub status: IntentStatus,
    /// Zero until recorded as executed.
    pub executed_at: u64,
}

#[contract]
pub struct ScholarshipDisbursementsContract;

#[contractimpl]
impl ScholarshipDisbursementsContract {
    /// One-time bootstrap. `network_id` is `sha256(network passphrase)` (the
    /// Stellar network id) and is committed into every wallet-ownership
    /// proof, so a testnet proof can never be replayed on mainnet.
    pub fn initialize(
        env: Env,
        admin: Address,
        network_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::NetworkId, &network_id);
        Ok(())
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if *caller != admin {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();
        Ok(())
    }

    /// Accepts the admin or an allowlisted disbursement creator.
    fn require_creator_or_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if *caller == admin || Self::is_authorized_creator(env.clone(), caller.clone()) {
            caller.require_auth();
            Ok(())
        } else {
            Err(ContractError::NotAuthorizedCreator)
        }
    }

    fn load_binding(env: &Env, recipient: &Address) -> Result<PayoutWallet, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::WalletBinding(recipient.clone()))
            .ok_or(ContractError::WalletNotFound)
    }

    fn save_binding(env: &Env, recipient: &Address, binding: &PayoutWallet) {
        let key = DataKey::WalletBinding(recipient.clone());
        env.storage().persistent().set(&key, binding);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    // ── #1096 — creator allowlist ─────────────────────────────────────────

    pub fn add_creator(
        env: Env,
        admin: Address,
        creator: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Creator(creator);
        env.storage().persistent().set(&key, &true);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }

    pub fn remove_creator(
        env: Env,
        admin: Address,
        creator: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .persistent()
            .set(&DataKey::Creator(creator), &false);
        Ok(())
    }

    pub fn is_authorized_creator(env: Env, creator: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Creator(creator))
            .unwrap_or(false)
    }

    // ── #1097 — wallet key registration + ownership proof ────────────────

    /// Bind an ed25519 public key to a wallet address. The wallet itself
    /// must authorize this, which is what makes the key↔address binding
    /// trustworthy: only someone controlling `wallet` can register its key.
    pub fn register_wallet_key(
        env: Env,
        wallet: Address,
        pubkey: BytesN<32>,
    ) -> Result<(), ContractError> {
        wallet.require_auth();
        let key = DataKey::WalletKey(wallet.clone());
        env.storage().persistent().set(&key, &pubkey);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events().publish((symbol_short!("WALLKEY"),), (wallet,));
        Ok(())
    }

    pub fn get_wallet_key(env: Env, wallet: Address) -> Result<BytesN<32>, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::WalletKey(wallet))
            .ok_or(ContractError::WalletKeyNotFound)
    }

    /// Set (or change) the recipient's payout wallet. The recipient must
    /// authorize this. Any change increments the wallet `epoch` and clears
    /// the verified flag, which places every intent created under a previous
    /// epoch on safe hold without having to enumerate them.
    pub fn set_payout_wallet(
        env: Env,
        recipient: Address,
        wallet: Address,
    ) -> Result<(), ContractError> {
        recipient.require_auth();

        let old: Option<PayoutWallet> = env
            .storage()
            .persistent()
            .get(&DataKey::WalletBinding(recipient.clone()));
        let epoch = old
            .map(|b| b.epoch)
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(ContractError::Overflow)?;

        let binding = PayoutWallet {
            wallet: wallet.clone(),
            verified: false,
            epoch,
            updated_at: env.ledger().timestamp(),
        };
        Self::save_binding(&env, &recipient, &binding);

        env.events()
            .publish((symbol_short!("WALLSET"),), (recipient, wallet, epoch));
        Ok(())
    }

    pub fn get_payout_wallet(env: Env, recipient: Address) -> Result<PayoutWallet, ContractError> {
        Self::load_binding(&env, &recipient)
    }

    pub fn is_wallet_verified(env: Env, recipient: Address) -> bool {
        match Self::load_binding(&env, &recipient) {
            Ok(b) => b.verified,
            Err(_) => false,
        }
    }

    fn wallet_proof_domain(env: &Env) -> Result<SigningDomain, ContractError> {
        let network_id: BytesN<32> = env
            .storage()
            .instance()
            .get(&DataKey::NetworkId)
            .ok_or(ContractError::NotInitialized)?;
        Ok(SigningDomain {
            version: ENVELOPE_VERSION,
            network_id,
            contract: env.current_contract_address(),
            message_type: MessageType::PayoutWalletProof,
        })
    }

    /// The exact, domain-separated 32-byte digest the wallet must sign for
    /// `challenge_id`. Public so an off-chain signer never has to
    /// re-implement the layout — it asks the contract for the payload.
    pub fn wallet_challenge_payload(
        env: Env,
        challenge_id: u64,
    ) -> Result<BytesN<32>, ContractError> {
        let challenge = Self::load_challenge(&env, challenge_id)?;
        let domain = Self::wallet_proof_domain(&env)?;
        signing::signing_payload(&env, &domain, &Self::challenge_body_hash(&env, &challenge))
            .map_err(|_| ContractError::InvalidSignature)
    }

    fn load_challenge(env: &Env, challenge_id: u64) -> Result<WalletChallenge, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Challenge(challenge_id))
            .ok_or(ContractError::ChallengeNotFound)
    }

    fn challenge_body_hash(env: &Env, challenge: &WalletChallenge) -> BytesN<32> {
        let mut input = Bytes::new(env);
        input.append(&Bytes::from_slice(env, WALLET_PROOF_DOMAIN_TAG));
        input.extend_from_array(&challenge.id.to_be_bytes());
        input.append(&challenge.recipient.clone().to_xdr(env));
        input.append(&challenge.wallet.clone().to_xdr(env));
        input.extend_from_array(&challenge.expires_at.to_be_bytes());
        env.crypto().sha256(&input).into()
    }

    /// Open a single-use, expiring challenge to prove control of the
    /// recipient's *currently configured* wallet. Only the wallet already
    /// configured via `set_payout_wallet` can be challenged, so a proof can
    /// never be gathered for an address the recipient did not choose.
    pub fn open_wallet_challenge(
        env: Env,
        recipient: Address,
        wallet: Address,
        ttl_seconds: u64,
    ) -> Result<u64, ContractError> {
        recipient.require_auth();

        if ttl_seconds == 0 || ttl_seconds > MAX_CHALLENGE_TTL_SECONDS {
            return Err(ContractError::InvalidChallengeTtl);
        }

        let binding = Self::load_binding(&env, &recipient)?;
        if binding.wallet != wallet {
            return Err(ContractError::WalletMismatch);
        }

        let now = env.ledger().timestamp();
        let expires_at = now
            .checked_add(ttl_seconds)
            .ok_or(ContractError::Overflow)?;

        let id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::ChallengeCounter)
            .unwrap_or(0);
        let next = id.checked_add(1).ok_or(ContractError::Overflow)?;
        env.storage().instance().set(&DataKey::ChallengeCounter, &next);

        let challenge = WalletChallenge {
            id,
            recipient: recipient.clone(),
            wallet: wallet.clone(),
            expires_at,
            used: false,
            created_at: now,
        };
        let key = DataKey::Challenge(id);
        env.storage().persistent().set(&key, &challenge);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("WCHALNGE"),),
            (id, recipient, wallet, expires_at),
        );

        Ok(id)
    }

    pub fn get_wallet_challenge(
        env: Env,
        challenge_id: u64,
    ) -> Result<WalletChallenge, ContractError> {
        Self::load_challenge(&env, challenge_id)
    }

    /// Confirm a wallet-ownership challenge with an ed25519 signature over
    /// the domain-separated payload. Anyone may relay this call — the
    /// signature, not the transaction signer, is the proof.
    ///
    /// Rejects a challenge that is unknown, already used, or expired; a
    /// wallet whose configured binding has since changed; a public key that
    /// does not match the one registered for the wallet; and any signature
    /// that does not verify. On success the recipient's wallet is marked
    /// verified and the challenge is consumed.
    pub fn confirm_wallet_challenge(
        env: Env,
        challenge_id: u64,
        pubkey: BytesN<32>,
        signature: BytesN<64>,
    ) -> Result<(), ContractError> {
        let mut challenge = Self::load_challenge(&env, challenge_id)?;

        if challenge.used {
            return Err(ContractError::ChallengeAlreadyUsed);
        }
        if env.ledger().timestamp() >= challenge.expires_at {
            return Err(ContractError::ChallengeExpired);
        }

        // The challenge must still target the recipient's configured wallet.
        let mut binding = Self::load_binding(&env, &challenge.recipient)?;
        if binding.wallet != challenge.wallet {
            return Err(ContractError::ChallengeWalletMismatch);
        }

        // The presented key must be the one registered for that wallet.
        let registered: BytesN<32> = env
            .storage()
            .persistent()
            .get(&DataKey::WalletKey(challenge.wallet.clone()))
            .ok_or(ContractError::WalletKeyNotFound)?;
        if registered != pubkey {
            return Err(ContractError::WalletKeyMismatch);
        }

        let domain = Self::wallet_proof_domain(&env)?;
        let body_hash = Self::challenge_body_hash(&env, &challenge);
        let payload = signing::signing_payload(&env, &domain, &body_hash)
            .map_err(|_| ContractError::InvalidSignature)?;
        verify_ed25519(&pubkey, &payload, &signature)?;

        challenge.used = true;
        let key = DataKey::Challenge(challenge_id);
        env.storage().persistent().set(&key, &challenge);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        binding.verified = true;
        binding.updated_at = env.ledger().timestamp();
        Self::save_binding(&env, &challenge.recipient, &binding);

        env.events().publish(
            (symbol_short!("WALLVERF"),),
            (challenge_id, challenge.recipient, challenge.wallet),
        );
        Ok(())
    }

    // ── #1096 — idempotent disbursement intents ───────────────────────────

    /// Deterministic, collision-resistant intent key for an installment.
    fn derive_intent_id(
        env: &Env,
        program_id: &BytesN<32>,
        recipient: &Address,
        installment: u32,
    ) -> BytesN<32> {
        let mut input = Bytes::new(env);
        input.append(&Bytes::from_slice(env, INTENT_DOMAIN_TAG));
        input.extend_from_array(&program_id.to_array());
        input.append(&recipient.clone().to_xdr(env));
        input.extend_from_array(&installment.to_be_bytes());
        env.crypto().sha256(&input).into()
    }

    /// Create (or reconcile) the single payment intent for an installment.
    ///
    /// The key is derived from `(program_id, recipient, installment)`, so:
    /// - a retry with identical parameters returns the existing intent and
    ///   changes no state (idempotent, "retries reconcile existing state");
    /// - a retry with a different `amount` (or a recipient wallet that no
    ///   longer matches the stored intent) is rejected with `IntentConflict`,
    ///   which is what makes amounts and recipients immutable.
    ///
    /// The intent targets the recipient's currently configured wallet and
    /// records its `epoch`, so a later wallet change pauses it.
    pub fn create_intent(
        env: Env,
        creator: Address,
        program_id: BytesN<32>,
        recipient: Address,
        amount: i128,
        installment: u32,
    ) -> Result<BytesN<32>, ContractError> {
        Self::require_creator_or_admin(&env, &creator)?;

        if amount <= 0 {
            return Err(ContractError::InvalidAmount);
        }

        let binding = Self::load_binding(&env, &recipient)?;
        let id = Self::derive_intent_id(&env, &program_id, &recipient, installment);
        let key = DataKey::Intent(id.clone());

        if let Some(existing) = env
            .storage()
            .persistent()
            .get::<DataKey, DisbursementIntent>(&key)
        {
            if existing.program_id == program_id
                && existing.recipient == recipient
                && existing.wallet == binding.wallet
                && existing.amount == amount
                && existing.installment == installment
            {
                return Ok(id);
            }
            return Err(ContractError::IntentConflict);
        }

        let intent = DisbursementIntent {
            id: id.clone(),
            program_id,
            recipient: recipient.clone(),
            wallet: binding.wallet,
            amount,
            installment,
            wallet_epoch: binding.epoch,
            created_at: env.ledger().timestamp(),
            created_by: creator,
            status: IntentStatus::Pending,
            executed_at: 0,
        };
        env.storage().persistent().set(&key, &intent);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events()
            .publish((symbol_short!("INTENTNW"),), (id.clone(), recipient, amount));

        Ok(id)
    }

    /// True only when the intent is `Pending` and its captured wallet is the
    /// recipient's *currently verified* wallet for the same epoch. This is
    /// the single source of truth for "can this be executed", so a wallet
    /// change puts prior intents on hold automatically.
    pub fn is_intent_executable(env: Env, intent_id: BytesN<32>) -> bool {
        let intent: Option<DisbursementIntent> = env
            .storage()
            .persistent()
            .get(&DataKey::Intent(intent_id));
        match intent {
            Some(i) if i.status == IntentStatus::Pending => {
                match Self::load_binding(&env, &i.recipient) {
                    Ok(b) => {
                        b.verified && b.epoch == i.wallet_epoch && b.wallet == i.wallet
                    }
                    Err(_) => false,
                }
            }
            _ => false,
        }
    }

    /// Record that an `Pending` intent has been (or is being) executed
    /// externally. Fails with `IntentOnHold` if the recipient's wallet is no
    /// longer the verified target, and with `AlreadyExecuted` on any retry —
    /// so an installment can be marked executed at most once.
    pub fn record_execution(
        env: Env,
        executor: Address,
        intent_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_creator_or_admin(&env, &executor)?;

        let key = DataKey::Intent(intent_id.clone());
        let mut intent: DisbursementIntent = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::IntentNotFound)?;

        match intent.status {
            IntentStatus::Executed => return Err(ContractError::AlreadyExecuted),
            IntentStatus::Cancelled => return Err(ContractError::AlreadyCancelled),
            IntentStatus::Pending => {}
        }

        if !Self::is_intent_executable(env.clone(), intent_id.clone()) {
            return Err(ContractError::IntentOnHold);
        }

        intent.status = IntentStatus::Executed;
        intent.executed_at = env.ledger().timestamp();
        env.storage().persistent().set(&key, &intent);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events()
            .publish((symbol_short!("INTENTEX"),), (intent_id,));
        Ok(())
    }

    /// Cancel a `Pending` intent that should not be paid (e.g. the recipient
    /// changed wallets and the old target is abandoned). The record is kept,
    /// never deleted, so history stays auditable.
    pub fn cancel_intent(
        env: Env,
        caller: Address,
        intent_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_creator_or_admin(&env, &caller)?;

        let key = DataKey::Intent(intent_id.clone());
        let mut intent: DisbursementIntent = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::IntentNotFound)?;

        match intent.status {
            IntentStatus::Executed => return Err(ContractError::AlreadyExecuted),
            IntentStatus::Cancelled => return Err(ContractError::AlreadyCancelled),
            IntentStatus::Pending => {}
        }

        intent.status = IntentStatus::Cancelled;
        env.storage().persistent().set(&key, &intent);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events()
            .publish((symbol_short!("INTENTCN"),), (intent_id,));
        Ok(())
    }

    pub fn get_intent(
        env: Env,
        intent_id: BytesN<32>,
    ) -> Result<DisbursementIntent, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Intent(intent_id))
            .ok_or(ContractError::IntentNotFound)
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

/// Verifies an ed25519 signature over `payload` using `pubkey`.
fn verify_ed25519(
    pubkey: &BytesN<32>,
    payload: &BytesN<32>,
    signature: &BytesN<64>,
) -> Result<(), ContractError> {
    let pubkey_arr: [u8; 32] = pubkey.to_array();
    let verifying_key = VerifyingKey::from_bytes(&pubkey_arr)
        .map_err(|_| ContractError::InvalidSignature)?;
    let signature = Signature::from_bytes(&signature.to_array());
    verifying_key
        .verify_strict(&payload.to_array(), &signature)
        .map_err(|_| ContractError::InvalidSignature)
}

#[cfg(test)]
mod tests;
