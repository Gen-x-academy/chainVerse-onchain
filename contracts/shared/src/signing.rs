//! Canonical domain-separated signing envelope (#1007).
//!
//! A signature the library accepts must mean exactly one thing. Without domain
//! separation, a signature gathered for a fine waiver on testnet is also a
//! valid signature for an issuer offer on mainnet against a different contract
//! — the bytes signed are the same, so the holder of one authorization holds
//! all of them.
//!
//! This module fixes the bytes. Every off-chain signature the library verifies
//! is taken over [`signing_payload`], which commits to five things the signer
//! cannot be assumed to have checked themselves:
//!
//! 1. a constant domain tag, so library payloads can never collide with any
//!    other ChainVerse signing scheme;
//! 2. an envelope **version**, so the layout can change without old signatures
//!    silently remaining valid under the new interpretation;
//! 3. the **network id** (`sha256(network passphrase)`, the Stellar network id),
//!    so a testnet signature is worthless on mainnet;
//! 4. the **contract address**, so a signature for one deployment does not work
//!    against another;
//! 5. the **message type**, so an issuer offer is not also a membership claim.
//!
//! ## Canonical preimage
//!
//! [`canonical_preimage`] returns exactly the bytes that get hashed. Every
//! field is fixed-width, so there is no length-prefix ambiguity and no way for
//! two different field sets to serialize to the same bytes.
//!
//! | Offset | Size | Field                                               |
//! |--------|------|-----------------------------------------------------|
//! | 0      | 18   | `DOMAIN_TAG` — the ASCII bytes `ChainVerse.Library`  |
//! | 18     | 4    | envelope version, `u32` big-endian                   |
//! | 22     | 32   | network id — `sha256(network passphrase)`            |
//! | 54     | 32   | `sha256(contract address XDR)`                       |
//! | 86     | 32   | `sha256(message type XDR)`                           |
//! | 118    | 32   | body hash — `sha256` of the message-specific payload |
//!
//! Total: [`PREIMAGE_LEN`] = 150 bytes.
//!
//! The address and message type are hashed rather than inlined because their
//! XDR encodings are variable-length; hashing them first keeps every field at a
//! fixed offset. A backend generating the same payload hashes the same 150
//! bytes, which is what makes the Rust and TypeScript fixtures comparable
//! byte-for-byte — [`canonical_preimage`] is public precisely so a fixture
//! generator can emit it rather than re-implementing the layout.
//!
//! ## Impact
//!
//! Additive library code in the `shared` crate. No ABI, storage, event,
//! privacy, deployment, or migration impact: no contract entrypoint changes and
//! no storage key is introduced. Adopting the envelope in a contract that
//! already verifies signatures **would** be a breaking change for existing
//! signatures, so [`ENVELOPE_VERSION`] exists to make that transition explicit.

use soroban_sdk::{contracterror, contracttype, xdr::ToXdr, Address, Bytes, BytesN, Env, Symbol};

/// Constant ASCII domain tag. 18 bytes.
pub const DOMAIN_TAG: &[u8; 18] = b"ChainVerse.Library";

/// The only envelope version this build accepts.
pub const ENVELOPE_VERSION: u32 = 1;

/// Length of [`canonical_preimage`]'s output, in bytes.
pub const PREIMAGE_LEN: u32 = 18 + 4 + 32 + 32 + 32 + 32;

/// Failures when building or checking a signing envelope.
///
/// Discriminants start at 220 so they cannot collide with [`crate::ContractError`]
/// or with `MathError`.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum SigningError {
    /// The envelope version is not one this build understands.
    UnsupportedVersion = 220,
    /// The recomputed payload does not match the one presented.
    DomainMismatch = 221,
}

/// What a signature is *for*. Part of the hashed preimage, so a signature for
/// one message type can never be replayed as another.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MessageType {
    /// A rights-holder's offer to license a work.
    IssuerOffer,
    /// A patron's claim to institutional membership.
    MembershipClaim,
    /// A librarian's assessment of a fine.
    FineAssessment,
    /// A scoped, time-limited delegation of reading rights.
    DelegatedSession,
    /// A patron's authorization to settle a debt from their deposit.
    DebtSettlement,
    /// A scholarship recipient's proof of control over a configured payout
    /// wallet (issue #1097). Scoped to this message type so a payout-wallet
    /// proof can never be replayed as any other signed action.
    PayoutWalletProof,
}

impl MessageType {
    /// A stable short name, used as the hashed message-type component.
    ///
    /// Spelled out rather than derived from the discriminant so that
    /// reordering the enum can never silently change a payload hash.
    pub fn tag(self, env: &Env) -> Symbol {
        match self {
            MessageType::IssuerOffer => Symbol::new(env, "issuer_offer"),
            MessageType::MembershipClaim => Symbol::new(env, "membership_claim"),
            MessageType::FineAssessment => Symbol::new(env, "fine_assessment"),
            MessageType::DelegatedSession => Symbol::new(env, "delegated_session"),
            MessageType::DebtSettlement => Symbol::new(env, "debt_settlement"),
            MessageType::PayoutWalletProof => Symbol::new(env, "payout_wallet_proof"),
        }
    }
}

/// The domain a signature is scoped to.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SigningDomain {
    /// Envelope version. Must equal [`ENVELOPE_VERSION`].
    pub version: u32,
    /// `sha256(network passphrase)` — the Stellar network id.
    pub network_id: BytesN<32>,
    /// The contract the signature is valid against.
    pub contract: Address,
    /// What the signature authorizes.
    pub message_type: MessageType,
}

/// Computes the Stellar network id from a network passphrase.
///
/// The passphrase is passed as its UTF-8 bytes, e.g.
/// `"Test SDF Network ; September 2015"`.
pub fn network_id(env: &Env, passphrase: &Bytes) -> BytesN<32> {
    env.crypto().sha256(passphrase).into()
}

/// Rejects an envelope whose version this build does not understand.
///
/// Called before anything else, so an old or forged version can never reach
/// the hashing path and produce a payload that looks legitimate.
pub fn check_version(domain: &SigningDomain) -> Result<(), SigningError> {
    if domain.version != ENVELOPE_VERSION {
        return Err(SigningError::UnsupportedVersion);
    }
    Ok(())
}

/// Builds the exact byte string that gets hashed. See the module docs for the
/// field layout.
///
/// Public so a fixture generator can emit the preimage itself, rather than a
/// second implementation of the layout drifting from this one.
pub fn canonical_preimage(
    env: &Env,
    domain: &SigningDomain,
    body_hash: &BytesN<32>,
) -> Result<Bytes, SigningError> {
    check_version(domain)?;

    let contract_hash: BytesN<32> = env
        .crypto()
        .sha256(&domain.contract.clone().to_xdr(env))
        .into();
    let message_type_hash: BytesN<32> = env
        .crypto()
        .sha256(&domain.message_type.tag(env).to_xdr(env))
        .into();

    let mut preimage = Bytes::new(env);
    preimage.extend_from_array(DOMAIN_TAG);
    preimage.extend_from_array(&domain.version.to_be_bytes());
    preimage.extend_from_array(&domain.network_id.to_array());
    preimage.extend_from_array(&contract_hash.to_array());
    preimage.extend_from_array(&message_type_hash.to_array());
    preimage.extend_from_array(&body_hash.to_array());

    Ok(preimage)
}

/// The 32-byte digest an off-chain signer signs.
pub fn signing_payload(
    env: &Env,
    domain: &SigningDomain,
    body_hash: &BytesN<32>,
) -> Result<BytesN<32>, SigningError> {
    let preimage = canonical_preimage(env, domain, body_hash)?;
    Ok(env.crypto().sha256(&preimage).into())
}

/// Confirms `presented` is the payload this domain and body actually produce.
///
/// Use this when a caller supplies the digest alongside the signature: it
/// closes the gap where a verifier checks a signature over bytes the caller
/// chose rather than bytes the contract derived.
pub fn assert_payload(
    env: &Env,
    domain: &SigningDomain,
    body_hash: &BytesN<32>,
    presented: &BytesN<32>,
) -> Result<(), SigningError> {
    if signing_payload(env, domain, body_hash)? != *presented {
        return Err(SigningError::DomainMismatch);
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env};

    const TESTNET: &[u8] = b"Test SDF Network ; September 2015";
    const PUBNET: &[u8] = b"Public Global Stellar Network ; September 2015";

    fn domain(env: &Env, passphrase: &[u8], contract: &Address, message_type: MessageType) -> SigningDomain {
        SigningDomain {
            version: ENVELOPE_VERSION,
            network_id: network_id(env, &Bytes::from_slice(env, passphrase)),
            contract: contract.clone(),
            message_type,
        }
    }

    fn body(env: &Env, fill: u8) -> BytesN<32> {
        BytesN::from_array(env, &[fill; 32])
    }

    // ── Layout ─────────────────────────────────────────────────────────────

    #[test]
    fn the_preimage_is_exactly_the_documented_length() {
        let env = Env::default();
        let contract = Address::generate(&env);
        let d = domain(&env, TESTNET, &contract, MessageType::IssuerOffer);

        let preimage = canonical_preimage(&env, &d, &body(&env, 1)).unwrap();
        assert_eq!(preimage.len(), PREIMAGE_LEN);
        assert_eq!(PREIMAGE_LEN, 150);
    }

    #[test]
    fn the_preimage_matches_the_documented_field_layout_byte_for_byte() {
        let env = Env::default();
        let contract = Address::generate(&env);
        let d = domain(&env, TESTNET, &contract, MessageType::FineAssessment);
        let body_hash = body(&env, 7);

        let actual = canonical_preimage(&env, &d, &body_hash).unwrap();

        // Rebuilt here from the documented table, independently of the
        // implementation, so a reordered or resized field fails this test.
        let contract_hash: BytesN<32> =
            env.crypto().sha256(&contract.clone().to_xdr(&env)).into();
        let message_type_hash: BytesN<32> = env
            .crypto()
            .sha256(&MessageType::FineAssessment.tag(&env).to_xdr(&env))
            .into();

        let mut expected = Bytes::new(&env);
        expected.extend_from_array(b"ChainVerse.Library");
        expected.extend_from_array(&1u32.to_be_bytes());
        expected.extend_from_array(&d.network_id.to_array());
        expected.extend_from_array(&contract_hash.to_array());
        expected.extend_from_array(&message_type_hash.to_array());
        expected.extend_from_array(&body_hash.to_array());

        assert_eq!(actual, expected);
    }

    #[test]
    fn the_domain_tag_occupies_the_first_eighteen_bytes() {
        let env = Env::default();
        let contract = Address::generate(&env);
        let d = domain(&env, TESTNET, &contract, MessageType::IssuerOffer);
        let preimage = canonical_preimage(&env, &d, &body(&env, 1)).unwrap();

        let tag = preimage.slice(0..18);
        assert_eq!(tag, Bytes::from_slice(&env, DOMAIN_TAG));
    }

    #[test]
    fn the_version_is_big_endian_at_offset_eighteen() {
        let env = Env::default();
        let contract = Address::generate(&env);
        let d = domain(&env, TESTNET, &contract, MessageType::IssuerOffer);
        let preimage = canonical_preimage(&env, &d, &body(&env, 1)).unwrap();

        assert_eq!(
            preimage.slice(18..22),
            Bytes::from_slice(&env, &1u32.to_be_bytes())
        );
    }

    // ── Determinism ────────────────────────────────────────────────────────

    #[test]
    fn the_same_inputs_always_produce_the_same_payload() {
        let env = Env::default();
        let contract = Address::generate(&env);
        let d = domain(&env, TESTNET, &contract, MessageType::MembershipClaim);
        let b = body(&env, 3);

        let first = signing_payload(&env, &d, &b).unwrap();
        let second = signing_payload(&env, &d, &b).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn a_different_body_produces_a_different_payload() {
        let env = Env::default();
        let contract = Address::generate(&env);
        let d = domain(&env, TESTNET, &contract, MessageType::MembershipClaim);

        assert_ne!(
            signing_payload(&env, &d, &body(&env, 1)).unwrap(),
            signing_payload(&env, &d, &body(&env, 2)).unwrap()
        );
    }

    // ── Cross-domain replay ────────────────────────────────────────────────

    #[test]
    fn a_testnet_payload_is_not_valid_on_mainnet() {
        let env = Env::default();
        let contract = Address::generate(&env);
        let b = body(&env, 5);

        let testnet = domain(&env, TESTNET, &contract, MessageType::IssuerOffer);
        let mainnet = domain(&env, PUBNET, &contract, MessageType::IssuerOffer);

        assert_ne!(testnet.network_id, mainnet.network_id);
        assert_ne!(
            signing_payload(&env, &testnet, &b).unwrap(),
            signing_payload(&env, &mainnet, &b).unwrap()
        );
    }

    #[test]
    fn a_payload_for_one_contract_is_not_valid_against_another() {
        let env = Env::default();
        let b = body(&env, 5);
        let first = Address::generate(&env);
        let second = Address::generate(&env);

        assert_ne!(
            signing_payload(
                &env,
                &domain(&env, TESTNET, &first, MessageType::IssuerOffer),
                &b
            )
            .unwrap(),
            signing_payload(
                &env,
                &domain(&env, TESTNET, &second, MessageType::IssuerOffer),
                &b
            )
            .unwrap()
        );
    }

    #[test]
    fn every_message_type_produces_a_distinct_payload() {
        let env = Env::default();
        let contract = Address::generate(&env);
        let b = body(&env, 5);

        let types = [
            MessageType::IssuerOffer,
            MessageType::MembershipClaim,
            MessageType::FineAssessment,
            MessageType::DelegatedSession,
            MessageType::DebtSettlement,
            MessageType::PayoutWalletProof,
        ];

        // Every pair must differ — an issuer offer must never double as a fine
        // assessment.
        for (i, left) in types.iter().enumerate() {
            for right in types.iter().skip(i + 1) {
                assert_ne!(
                    signing_payload(&env, &domain(&env, TESTNET, &contract, *left), &b).unwrap(),
                    signing_payload(&env, &domain(&env, TESTNET, &contract, *right), &b).unwrap(),
                    "{left:?} and {right:?} share a payload"
                );
            }
        }
    }

    #[test]
    fn a_payload_from_a_future_version_would_differ_from_this_ones() {
        let env = Env::default();
        let contract = Address::generate(&env);
        let b = body(&env, 5);
        let d = domain(&env, TESTNET, &contract, MessageType::IssuerOffer);

        // Build a v2 preimage by hand: the version field is the only change.
        let contract_hash: BytesN<32> =
            env.crypto().sha256(&contract.clone().to_xdr(&env)).into();
        let message_type_hash: BytesN<32> = env
            .crypto()
            .sha256(&MessageType::IssuerOffer.tag(&env).to_xdr(&env))
            .into();
        let mut v2 = Bytes::new(&env);
        v2.extend_from_array(DOMAIN_TAG);
        v2.extend_from_array(&2u32.to_be_bytes());
        v2.extend_from_array(&d.network_id.to_array());
        v2.extend_from_array(&contract_hash.to_array());
        v2.extend_from_array(&message_type_hash.to_array());
        v2.extend_from_array(&b.to_array());
        let v2_payload: BytesN<32> = env.crypto().sha256(&v2).into();

        assert_ne!(signing_payload(&env, &d, &b).unwrap(), v2_payload);
    }

    // ── Version gating ─────────────────────────────────────────────────────

    #[test]
    fn an_unknown_version_is_rejected_before_anything_is_hashed() {
        let env = Env::default();
        let contract = Address::generate(&env);
        let mut d = domain(&env, TESTNET, &contract, MessageType::IssuerOffer);
        d.version = ENVELOPE_VERSION + 1;

        assert_eq!(check_version(&d), Err(SigningError::UnsupportedVersion));
        assert_eq!(
            canonical_preimage(&env, &d, &body(&env, 1)),
            Err(SigningError::UnsupportedVersion)
        );
        assert_eq!(
            signing_payload(&env, &d, &body(&env, 1)),
            Err(SigningError::UnsupportedVersion)
        );
    }

    #[test]
    fn version_zero_is_rejected() {
        let env = Env::default();
        let contract = Address::generate(&env);
        let mut d = domain(&env, TESTNET, &contract, MessageType::IssuerOffer);
        d.version = 0;

        assert_eq!(
            signing_payload(&env, &d, &body(&env, 1)),
            Err(SigningError::UnsupportedVersion)
        );
    }

    // ── assert_payload ─────────────────────────────────────────────────────

    #[test]
    fn assert_payload_accepts_the_derived_digest() {
        let env = Env::default();
        let contract = Address::generate(&env);
        let d = domain(&env, TESTNET, &contract, MessageType::DebtSettlement);
        let b = body(&env, 9);
        let payload = signing_payload(&env, &d, &b).unwrap();

        assert_eq!(assert_payload(&env, &d, &b, &payload), Ok(()));
    }

    #[test]
    fn assert_payload_rejects_a_digest_from_another_domain() {
        let env = Env::default();
        let contract = Address::generate(&env);
        let b = body(&env, 9);

        let testnet = domain(&env, TESTNET, &contract, MessageType::DebtSettlement);
        let mainnet = domain(&env, PUBNET, &contract, MessageType::DebtSettlement);
        let mainnet_payload = signing_payload(&env, &mainnet, &b).unwrap();

        assert_eq!(
            assert_payload(&env, &testnet, &b, &mainnet_payload),
            Err(SigningError::DomainMismatch)
        );
    }

    #[test]
    fn assert_payload_rejects_a_digest_for_a_different_body() {
        let env = Env::default();
        let contract = Address::generate(&env);
        let d = domain(&env, TESTNET, &contract, MessageType::DebtSettlement);
        let other = signing_payload(&env, &d, &body(&env, 1)).unwrap();

        assert_eq!(
            assert_payload(&env, &d, &body(&env, 2), &other),
            Err(SigningError::DomainMismatch)
        );
    }

    // ── Network id ─────────────────────────────────────────────────────────

    #[test]
    fn the_network_id_is_the_sha256_of_the_passphrase() {
        let env = Env::default();
        let passphrase = Bytes::from_slice(&env, TESTNET);
        let expected: BytesN<32> = env.crypto().sha256(&passphrase).into();

        assert_eq!(network_id(&env, &passphrase), expected);
    }

    #[test]
    fn distinct_passphrases_give_distinct_network_ids() {
        let env = Env::default();
        assert_ne!(
            network_id(&env, &Bytes::from_slice(&env, TESTNET)),
            network_id(&env, &Bytes::from_slice(&env, PUBNET))
        );
    }
}
