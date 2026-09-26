//! Canonical signing envelope for sponsor webhooks (issue #1141).
//!
//! A sponsor's endpoint is an HTTP receiver, so the bytes that cross the
//! wire are produced off-chain and the chain never sees them. That is
//! precisely the case where a signature scheme has to be pinned down
//! exactly: if the relay is free to choose what it signs, then "signed by
//! the platform" means nothing, and a captured signature is replayable
//! against any endpoint that happens to accept it.
//!
//! This module fixes the bytes. [`signing_payload`] is the single source
//! of truth for what a delivery signature commits to, and it is public so
//! a backend and a sponsor's verifier derive the *same* digest rather than
//! two implementations drifting apart.
//!
//! ## Why this is not `shared::signing`
//!
//! The library's envelope ([`shared::signing`], domain tag
//! `ChainVerse.Library`) is a different scheme for a different domain, and
//! reusing it here would be a mistake in both directions: a library
//! signature would become a valid webhook signature, and the library would
//! have to grow a scholarship message type it has no use for. The
//! scholarship platform is deliberately independent of the library (ADR
//! 0002), so this envelope has its own tag and its own layout.
//! `cross_domain_signatures_differ` pins the separation down with a test.
//!
//! ## Canonical preimage
//!
//! Every field is fixed-width, so there is no length-prefix ambiguity and
//! no way for two different field sets to serialize to the same bytes.
//!
//! | Offset | Size | Field                                                |
//! |--------|------|------------------------------------------------------|
//! | 0      | 25   | `DOMAIN_TAG` — `ChainVerse.ScholarWebhook`            |
//! | 25     | 4    | envelope version, `u32` big-endian                    |
//! | 29     | 32   | network id — `sha256(network passphrase)`             |
//! | 61     | 32   | `sha256(contract address XDR)`                        |
//! | 93     | 8    | endpoint id, `u64` big-endian                         |
//! | 101    | 4    | secret epoch, `u32` big-endian                        |
//! | 105    | 8    | event id, `u64` big-endian                            |
//! | 113    | 8    | sequence, `u64` big-endian                            |
//! | 121    | 8    | expiry, `u64` big-endian ledger seconds               |
//! | 129    | 32   | payload hash — `sha256` of the delivered body         |
//!
//! Each of the mutable-looking fields is there for a specific attack:
//! `endpoint_id` and `secret_epoch` mean a signature captured for one
//! endpoint (or under a since-rotated secret) cannot be presented to
//! another; `sequence` and `expiry` bound replay; `event_id` and
//! `payload_hash` mean it cannot be moved to a different event or have its
//! body swapped. The network id and contract address stop a testnet
//! signature from being worth anything on mainnet, or against another
//! deployment.

use soroban_sdk::{xdr::ToXdr, Address, Bytes, BytesN, Env};

/// Constant ASCII domain tag. 25 bytes.
pub const DOMAIN_TAG: &[u8; 25] = b"ChainVerse.ScholarWebhook";

/// The only envelope version this build accepts.
pub const ENVELOPE_VERSION: u32 = 1;

/// Length of [`canonical_preimage`]'s output, in bytes.
///
/// Computed rather than written down, so adding or removing a field cannot
/// leave a stale constant behind.
pub const PREIMAGE_LEN: u32 = DOMAIN_TAG.len() as u32 + 4 + 32 + 32 + 8 + 4 + 8 + 8 + 8 + 32;

/// Everything a delivery signature commits to, except the body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebhookEnvelope {
    /// Envelope version. Must equal [`ENVELOPE_VERSION`].
    pub version: u32,
    /// The Stellar network id, taken from the ledger rather than from a
    /// caller. See `delivery_payload` for why that distinction matters.
    pub network_id: BytesN<32>,
    /// The outbox contract the signature is valid against.
    pub contract: Address,
    /// Which endpoint the delivery is for.
    pub endpoint_id: u64,
    /// Which generation of the endpoint's secret produced it. A rotation
    /// bumps this, which is what invalidates signatures captured under the
    /// old secret.
    pub secret_epoch: u32,
    /// The event being delivered.
    pub event_id: u64,
    /// The endpoint's strictly-increasing delivery counter.
    pub sequence: u64,
    /// Ledger second after which the signature is no longer acceptable.
    pub expires_at: u64,
}

/// The exact bytes that get hashed. See the module docs for the layout.
pub fn canonical_preimage(
    env: &Env,
    envelope: &WebhookEnvelope,
    payload_hash: &BytesN<32>,
) -> Bytes {
    // The address XDR is variable-length, so it is hashed to keep every
    // field at a fixed offset.
    let contract_hash: BytesN<32> = env
        .crypto()
        .sha256(&envelope.contract.clone().to_xdr(env))
        .into();

    let mut preimage = Bytes::new(env);
    preimage.extend_from_array(DOMAIN_TAG);
    preimage.extend_from_array(&envelope.version.to_be_bytes());
    preimage.extend_from_array(&envelope.network_id.to_array());
    preimage.extend_from_array(&contract_hash.to_array());
    preimage.extend_from_array(&envelope.endpoint_id.to_be_bytes());
    preimage.extend_from_array(&envelope.secret_epoch.to_be_bytes());
    preimage.extend_from_array(&envelope.event_id.to_be_bytes());
    preimage.extend_from_array(&envelope.sequence.to_be_bytes());
    preimage.extend_from_array(&envelope.expires_at.to_be_bytes());
    preimage.extend_from_array(&payload_hash.to_array());
    preimage
}

/// The 32-byte digest a relay signs and a sponsor verifies.
pub fn signing_payload(
    env: &Env,
    envelope: &WebhookEnvelope,
    payload_hash: &BytesN<32>,
) -> BytesN<32> {
    let preimage = canonical_preimage(env, envelope, payload_hash);
    env.crypto().sha256(&preimage).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger as _};

    const TESTNET: [u8; 32] = [1; 32];
    const PUBNET: [u8; 32] = [2; 32];

    fn envelope(env: &Env, contract: &Address) -> WebhookEnvelope {
        WebhookEnvelope {
            version: ENVELOPE_VERSION,
            network_id: env.ledger().network_id(),
            contract: contract.clone(),
            endpoint_id: 7,
            secret_epoch: 1,
            event_id: 42,
            sequence: 9,
            expires_at: 1_700_000_000,
        }
    }

    fn body(fill: u8) -> [u8; 32] {
        [fill; 32]
    }

    // ── Layout ─────────────────────────────────────────────────────────────

    #[test]
    fn the_preimage_is_the_documented_length() {
        let env = Env::default();
        let c = Address::generate(&env);
        let preimage = canonical_preimage(
            &env,
            &envelope(&env, &c),
            &BytesN::from_array(&env, &body(1)),
        );
        assert_eq!(preimage.len(), PREIMAGE_LEN);
    }

    #[test]
    fn the_preimage_starts_with_the_domain_tag() {
        let env = Env::default();
        let c = Address::generate(&env);
        let preimage = canonical_preimage(
            &env,
            &envelope(&env, &c),
            &BytesN::from_array(&env, &body(1)),
        );
        let tag: Bytes = preimage.slice(0..DOMAIN_TAG.len() as u32);
        assert_eq!(tag, Bytes::from_slice(&env, DOMAIN_TAG));
    }

    // ── Every field is committed to ────────────────────────────────────────
    //
    // One test per field, because a field that is not in the preimage is a
    // field an attacker gets to choose. A single "changing anything changes
    // the digest" test would not catch a field dropped from the layout.

    #[test]
    fn every_field_changes_the_digest() {
        let env = Env::default();
        let a = Address::generate(&env);
        let b = Address::generate(&env);
        let base = envelope(&env, &a);
        let hash = BytesN::from_array(&env, &body(1));
        let baseline = signing_payload(&env, &base, &hash);

        let mutates: [(&str, WebhookEnvelope, BytesN<32>); 9] = [
            (
                "version",
                WebhookEnvelope {
                    version: 2,
                    ..base.clone()
                },
                hash.clone(),
            ),
            (
                "network_id",
                WebhookEnvelope {
                    network_id: BytesN::from_array(&env, &PUBNET),
                    ..base.clone()
                },
                hash.clone(),
            ),
            (
                "contract",
                WebhookEnvelope {
                    contract: b.clone(),
                    ..base.clone()
                },
                hash.clone(),
            ),
            (
                "endpoint_id",
                WebhookEnvelope {
                    endpoint_id: 8,
                    ..base.clone()
                },
                hash.clone(),
            ),
            (
                "secret_epoch",
                WebhookEnvelope {
                    secret_epoch: 2,
                    ..base.clone()
                },
                hash.clone(),
            ),
            (
                "event_id",
                WebhookEnvelope {
                    event_id: 43,
                    ..base.clone()
                },
                hash.clone(),
            ),
            (
                "sequence",
                WebhookEnvelope {
                    sequence: 10,
                    ..base.clone()
                },
                hash.clone(),
            ),
            (
                "expires_at",
                WebhookEnvelope {
                    expires_at: 1_700_000_001,
                    ..base.clone()
                },
                hash.clone(),
            ),
            ("payload", base.clone(), BytesN::from_array(&env, &body(2))),
        ];

        for (field, e, h) in mutates.iter() {
            assert_ne!(
                signing_payload(&env, e, h),
                baseline,
                "changing {field} must change the digest"
            );
        }
    }

    // ── Cross-domain separation ────────────────────────────────────────────

    #[test]
    fn cross_domain_signatures_differ() {
        let env = Env::default();
        let c = Address::generate(&env);
        let web = envelope(&env, &c);
        let hash = BytesN::from_array(&env, &body(1));
        let web_digest = signing_payload(&env, &web, &hash);

        // Rebuild the same logical envelope under the library's tag and
        // layout. Identical inputs, different domain: the digests must not
        // agree, or a library signature would verify as a webhook one.
        let mut lib = Bytes::new(&env);
        lib.extend_from_array(b"ChainVerse.Library");
        lib.extend_from_array(&1u32.to_be_bytes());
        lib.extend_from_array(&web.network_id.to_array());
        lib.extend_from_array(&env.crypto().sha256(&c.to_xdr(&env)).to_array());
        lib.extend_from_array(&[0u8; 32]);
        lib.extend_from_array(&hash.to_array());
        let lib_digest: BytesN<32> = env.crypto().sha256(&lib).into();

        assert_ne!(web_digest, lib_digest);
    }

    // ── Stability ──────────────────────────────────────────────────────────

    #[test]
    fn the_digest_is_deterministic() {
        let env = Env::default();
        let c = Address::generate(&env);
        let e = envelope(&env, &c);
        let h = BytesN::from_array(&env, &body(7));
        assert_eq!(signing_payload(&env, &e, &h), signing_payload(&env, &e, &h));
    }

    #[test]
    fn a_different_network_yields_a_different_digest() {
        let env = Env::default();
        let c = Address::generate(&env);
        let h = BytesN::from_array(&env, &body(7));

        env.ledger().set_network_id(TESTNET);
        let on_testnet = signing_payload(&env, &envelope(&env, &c), &h);
        env.ledger().set_network_id(PUBNET);
        let on_pubnet = signing_payload(&env, &envelope(&env, &c), &h);

        assert_ne!(on_testnet, on_pubnet);
    }

    #[test]
    fn a_different_deployment_yields_a_different_digest() {
        let env = Env::default();
        let a = Address::generate(&env);
        let b = Address::generate(&env);
        let h = BytesN::from_array(&env, &body(7));
        assert_ne!(
            signing_payload(&env, &envelope(&env, &a), &h),
            signing_payload(&env, &envelope(&env, &b), &h)
        );
    }
}
