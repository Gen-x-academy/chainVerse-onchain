//! Salted commitments for low-entropy identifiers (#1005).
//!
//! An ISBN has about 30 bits of entropy and there are only a few million books
//! in print. A student ID at a given institution is usually a six- to
//! nine-digit number. A library barcode is sequential. Writing the `sha256` of
//! any of these to a public ledger is not privacy — the entire keyspace can be
//! enumerated on a laptop in seconds, so the digest is equivalent to the
//! plaintext.
//!
//! This module defines the keyed, domain-separated commitment the library uses
//! instead, and the check that stops a raw identifier reaching storage in the
//! first place.
//!
//! ## Commitment construction
//!
//! ```text
//! commitment = sha256(
//!       COMMITMENT_TAG            // 24 bytes, constant
//!    || kind                      // 4 bytes, big-endian
//!    || salt_version              // 4 bytes, big-endian
//!    || institution_salt          // 32 bytes, secret, per institution
//!    || sha256(identifier)        // 32 bytes
//! )
//! ```
//!
//! The salt is the only secret. Everything else is public and fixed-width, so
//! two different field sets can never produce the same preimage.
//!
//! Domain separation happens on three axes at once:
//!
//! * **kind** — the same digits as an ISBN and as a student ID commit
//!   differently, so a value cannot be confused across identifier spaces;
//! * **institution** — the same student ID at two institutions is
//!   uncorrelatable, which is what stops cross-institution tracking;
//! * **salt version** — rotating the salt re-randomizes every commitment.
//!
//! ## Why a 32-byte salt
//!
//! The salt has to defeat an offline dictionary attack over the whole
//! identifier space, so it must be long enough that guessing it is harder than
//! guessing the identifier — which, at ~30 bits, is no bar at all. 256 bits is
//! the same width as the digest and costs nothing extra to store.
//!
//! ## Salt rotation and migration
//!
//! Rotation is a re-commitment, not an edit. A commitment is a one-way
//! function of a secret the contract never sees, so the contract cannot
//! recompute old commitments under a new salt. The procedure is:
//!
//! 1. The institution generates salt version `n + 1` off-chain.
//! 2. It recomputes every commitment it holds under the new version.
//! 3. It submits the re-commitments alongside the new `salt_version`.
//! 4. The contract stores both versions during a documented overlap window,
//!    then drops version `n`.
//!
//! Because `salt_version` is inside the preimage, a stale commitment simply
//! fails to match after the overlap closes — there is no path where an old
//! commitment silently keeps working.
//!
//! ## Impact
//!
//! Additive library code in the `shared` crate. No ABI, storage, event,
//! privacy, deployment, or migration impact on its own. Adopting
//! [`reject_raw_identifier`] in an entrypoint that currently accepts a raw
//! identifier **is** a breaking change for existing callers, and rotating a
//! salt invalidates every commitment under the previous version — both belong
//! in their own change with a documented cutover.

use soroban_sdk::{contracterror, contracttype, Bytes, BytesN, Env};

/// Constant domain tag. 24 bytes.
pub const COMMITMENT_TAG: &[u8; 24] = b"ChainVerse.Library.Ident";

/// Required institution salt width, in bytes.
pub const SALT_LEN: u32 = 32;

/// Length of [`commitment_preimage`]'s output, in bytes.
pub const PREIMAGE_LEN: u32 = 24 + 4 + 4 + 32 + 32;

/// Identifiers shorter than this are always treated as low-entropy.
const MIN_OPAQUE_LEN: u32 = 16;

/// Failures when committing or screening an identifier.
///
/// Discriminants start at 260 so they cannot collide with `ContractError`
/// (1–15), `MathError` (200+), `SigningError` (220+), or `ConfigError` (240+).
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum CommitmentError {
    /// The institution salt is not [`SALT_LEN`] bytes.
    InvalidSalt = 260,
    /// The salt is all zero bytes — an unset or placeholder value.
    UnseededSalt = 261,
    /// A raw, enumerable identifier was submitted where a commitment is
    /// required.
    RawIdentifierRejected = 262,
    /// The identifier was empty.
    EmptyIdentifier = 263,
    /// Salt version zero is reserved for "unset".
    InvalidSaltVersion = 264,
}

/// Which identifier space a value belongs to.
///
/// Part of the commitment preimage, so the same digits in two spaces commit
/// differently.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum IdentifierKind {
    /// ISBN-10 or ISBN-13.
    Isbn = 1,
    /// A physical-item barcode, typically sequential.
    Barcode = 2,
    /// An institution-issued patron number.
    StudentId = 3,
    /// A patron's pseudonymous library identifier.
    PatronId = 4,
    /// A hash of an off-ledger document.
    DocumentHash = 5,
    /// A reading-list or collection reference.
    ReadingList = 6,
}

impl IdentifierKind {
    /// Stable numeric tag used in the preimage.
    ///
    /// Spelled out rather than taken from the discriminant so that reordering
    /// the enum cannot silently change every commitment in existence.
    pub fn tag(self) -> u32 {
        match self {
            IdentifierKind::Isbn => 1,
            IdentifierKind::Barcode => 2,
            IdentifierKind::StudentId => 3,
            IdentifierKind::PatronId => 4,
            IdentifierKind::DocumentHash => 5,
            IdentifierKind::ReadingList => 6,
        }
    }
}

/// Validates an institution salt.
///
/// Rejects the all-zero salt explicitly: an unset storage slot reads as zeros,
/// and a salt of zeros provides no protection at all while looking like it
/// does.
pub fn validate_salt(salt: &Bytes) -> Result<(), CommitmentError> {
    if salt.len() != SALT_LEN {
        return Err(CommitmentError::InvalidSalt);
    }
    let mut all_zero = true;
    for byte in salt.iter() {
        if byte != 0 {
            all_zero = false;
            break;
        }
    }
    if all_zero {
        return Err(CommitmentError::UnseededSalt);
    }
    Ok(())
}

/// Screens a value that is about to be stored or emitted.
///
/// Returns [`CommitmentError::RawIdentifierRejected`] for anything that looks
/// like a plaintext enumerable identifier:
///
/// * shorter than 16 bytes — too small to be a commitment at all;
/// * all ASCII digits, optionally with `-` or spaces — an ISBN, a barcode, or
///   a student number.
///
/// This is a guard rail, not a proof. It cannot detect a high-entropy-looking
/// value that is in fact drawn from a small set; that is what the commitment
/// construction is for. What it does catch is the common mistake of passing
/// the identifier straight through, which is the failure actually seen in
/// practice.
pub fn reject_raw_identifier(value: &Bytes) -> Result<(), CommitmentError> {
    if value.is_empty() {
        return Err(CommitmentError::EmptyIdentifier);
    }
    if value.len() < MIN_OPAQUE_LEN {
        return Err(CommitmentError::RawIdentifierRejected);
    }

    let mut digits = 0u32;
    let mut other = 0u32;
    for byte in value.iter() {
        if byte.is_ascii_digit() {
            digits += 1;
        } else if byte != b'-' && byte != b' ' {
            // Separators are tolerated inside a printed identifier and are
            // neither digits nor evidence of opacity.
            other += 1;
        }
    }

    // Digits and separators only, with at least one digit: a printed
    // identifier, not a commitment.
    if other == 0 && digits > 0 {
        return Err(CommitmentError::RawIdentifierRejected);
    }

    Ok(())
}

/// Builds the exact bytes that get hashed into a commitment. See the module
/// docs for the layout.
///
/// Public so an institution's off-chain tooling can produce byte-identical
/// commitments without re-implementing the layout.
pub fn commitment_preimage(
    env: &Env,
    kind: IdentifierKind,
    salt_version: u32,
    institution_salt: &Bytes,
    identifier: &Bytes,
) -> Result<Bytes, CommitmentError> {
    if salt_version == 0 {
        return Err(CommitmentError::InvalidSaltVersion);
    }
    validate_salt(institution_salt)?;
    if identifier.is_empty() {
        return Err(CommitmentError::EmptyIdentifier);
    }

    let identifier_hash: BytesN<32> = env.crypto().sha256(identifier).into();
    let mut salt_bytes = [0u8; SALT_LEN as usize];
    institution_salt.copy_into_slice(&mut salt_bytes);

    let mut preimage = Bytes::new(env);
    preimage.extend_from_array(COMMITMENT_TAG);
    preimage.extend_from_array(&kind.tag().to_be_bytes());
    preimage.extend_from_array(&salt_version.to_be_bytes());
    preimage.extend_from_array(&salt_bytes);
    preimage.extend_from_array(&identifier_hash.to_array());

    Ok(preimage)
}

/// The commitment an institution stores in place of a raw identifier.
pub fn commit(
    env: &Env,
    kind: IdentifierKind,
    salt_version: u32,
    institution_salt: &Bytes,
    identifier: &Bytes,
) -> Result<BytesN<32>, CommitmentError> {
    let preimage = commitment_preimage(env, kind, salt_version, institution_salt, identifier)?;
    Ok(env.crypto().sha256(&preimage).into())
}

/// Confirms `presented` is the commitment this identifier actually produces
/// under the institution's salt.
pub fn verify(
    env: &Env,
    kind: IdentifierKind,
    salt_version: u32,
    institution_salt: &Bytes,
    identifier: &Bytes,
    presented: &BytesN<32>,
) -> Result<bool, CommitmentError> {
    Ok(commit(env, kind, salt_version, institution_salt, identifier)? == *presented)
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::Env;

    fn salt(env: &Env, fill: u8) -> Bytes {
        Bytes::from_slice(env, &[fill; 32])
    }

    fn ident(env: &Env, s: &[u8]) -> Bytes {
        Bytes::from_slice(env, s)
    }

    // ── Salt validation ────────────────────────────────────────────────────

    #[test]
    fn a_salt_must_be_exactly_thirty_two_bytes() {
        let env = Env::default();
        assert!(validate_salt(&salt(&env, 1)).is_ok());
        assert_eq!(
            validate_salt(&Bytes::from_slice(&env, &[1u8; 31])),
            Err(CommitmentError::InvalidSalt)
        );
        assert_eq!(
            validate_salt(&Bytes::from_slice(&env, &[1u8; 33])),
            Err(CommitmentError::InvalidSalt)
        );
        assert_eq!(
            validate_salt(&Bytes::new(&env)),
            Err(CommitmentError::InvalidSalt)
        );
    }

    #[test]
    fn an_all_zero_salt_is_rejected_as_unseeded() {
        let env = Env::default();
        assert_eq!(
            validate_salt(&salt(&env, 0)),
            Err(CommitmentError::UnseededSalt)
        );
        // A single non-zero byte is enough to clear the unseeded check, though
        // it is obviously still a terrible salt — that is an operational
        // concern, not something the contract can measure.
        let mut nearly_zero = [0u8; 32];
        nearly_zero[31] = 1;
        assert!(validate_salt(&Bytes::from_slice(&env, &nearly_zero)).is_ok());
    }

    // ── Raw identifier screening ───────────────────────────────────────────

    #[test]
    fn a_bare_isbn_is_rejected() {
        let env = Env::default();
        assert_eq!(
            reject_raw_identifier(&ident(&env, b"9780262033848")),
            Err(CommitmentError::RawIdentifierRejected)
        );
        assert_eq!(
            reject_raw_identifier(&ident(&env, b"978-0-262-03384-8")),
            Err(CommitmentError::RawIdentifierRejected)
        );
    }

    #[test]
    fn a_bare_barcode_or_student_number_is_rejected() {
        let env = Env::default();
        assert_eq!(
            reject_raw_identifier(&ident(&env, b"31234567890123")),
            Err(CommitmentError::RawIdentifierRejected)
        );
        assert_eq!(
            reject_raw_identifier(&ident(&env, b"00123456")),
            Err(CommitmentError::RawIdentifierRejected)
        );
    }

    #[test]
    fn a_short_value_is_rejected_whatever_it_contains() {
        let env = Env::default();
        // Too short to be a 32-byte commitment, so it cannot be one.
        assert_eq!(
            reject_raw_identifier(&ident(&env, b"abcdef")),
            Err(CommitmentError::RawIdentifierRejected)
        );
    }

    #[test]
    fn an_empty_value_is_rejected_as_empty_not_as_raw() {
        let env = Env::default();
        assert_eq!(
            reject_raw_identifier(&Bytes::new(&env)),
            Err(CommitmentError::EmptyIdentifier)
        );
    }

    #[test]
    fn a_thirty_two_byte_commitment_passes_screening() {
        let env = Env::default();
        let commitment = commit(
            &env,
            IdentifierKind::Isbn,
            1,
            &salt(&env, 7),
            &ident(&env, b"9780262033848"),
        )
        .unwrap();

        assert!(reject_raw_identifier(&Bytes::from_array(&env, &commitment.to_array())).is_ok());
    }

    #[test]
    fn a_long_digits_only_string_is_still_rejected() {
        let env = Env::default();
        // 32 characters, but every one of them a digit: still enumerable.
        assert_eq!(
            reject_raw_identifier(&ident(&env, b"12345678901234567890123456789012")),
            Err(CommitmentError::RawIdentifierRejected)
        );
    }

    // ── Preimage layout ────────────────────────────────────────────────────

    #[test]
    fn the_preimage_is_exactly_the_documented_length() {
        let env = Env::default();
        let p = commitment_preimage(
            &env,
            IdentifierKind::Isbn,
            1,
            &salt(&env, 3),
            &ident(&env, b"9780262033848"),
        )
        .unwrap();

        assert_eq!(p.len(), PREIMAGE_LEN);
        assert_eq!(PREIMAGE_LEN, 96);
    }

    #[test]
    fn the_preimage_matches_the_documented_field_layout_byte_for_byte() {
        let env = Env::default();
        let institution_salt = salt(&env, 3);
        let identifier = ident(&env, b"9780262033848");

        let actual = commitment_preimage(
            &env,
            IdentifierKind::StudentId,
            4,
            &institution_salt,
            &identifier,
        )
        .unwrap();

        // Rebuilt from the documented table, independently of the
        // implementation, so a reordered or resized field fails here.
        let identifier_hash: BytesN<32> = env.crypto().sha256(&identifier).into();
        let mut expected = Bytes::new(&env);
        expected.extend_from_array(b"ChainVerse.Library.Ident");
        expected.extend_from_array(&3u32.to_be_bytes()); // StudentId tag
        expected.extend_from_array(&4u32.to_be_bytes()); // salt version
        expected.extend_from_array(&[3u8; 32]);
        expected.extend_from_array(&identifier_hash.to_array());

        assert_eq!(actual, expected);
    }

    #[test]
    fn salt_version_zero_is_reserved_and_rejected() {
        let env = Env::default();
        assert_eq!(
            commit(
                &env,
                IdentifierKind::Isbn,
                0,
                &salt(&env, 1),
                &ident(&env, b"9780262033848")
            ),
            Err(CommitmentError::InvalidSaltVersion)
        );
    }

    #[test]
    fn committing_an_empty_identifier_is_rejected() {
        let env = Env::default();
        assert_eq!(
            commit(&env, IdentifierKind::Isbn, 1, &salt(&env, 1), &Bytes::new(&env)),
            Err(CommitmentError::EmptyIdentifier)
        );
    }

    #[test]
    fn committing_under_an_invalid_salt_is_rejected() {
        let env = Env::default();
        let id = ident(&env, b"9780262033848");
        assert_eq!(
            commit(&env, IdentifierKind::Isbn, 1, &salt(&env, 0), &id),
            Err(CommitmentError::UnseededSalt)
        );
        assert_eq!(
            commit(
                &env,
                IdentifierKind::Isbn,
                1,
                &Bytes::from_slice(&env, &[1u8; 16]),
                &id
            ),
            Err(CommitmentError::InvalidSalt)
        );
    }

    // ── Domain separation: the test vectors ────────────────────────────────

    #[test]
    fn the_same_inputs_always_commit_to_the_same_value() {
        let env = Env::default();
        let s = salt(&env, 5);
        let id = ident(&env, b"9780262033848");

        assert_eq!(
            commit(&env, IdentifierKind::Isbn, 1, &s, &id).unwrap(),
            commit(&env, IdentifierKind::Isbn, 1, &s, &id).unwrap()
        );
    }

    #[test]
    fn the_same_digits_commit_differently_in_different_identifier_spaces() {
        let env = Env::default();
        let s = salt(&env, 5);
        // A number that could plausibly be either.
        let id = ident(&env, b"31234567890123");

        assert_ne!(
            commit(&env, IdentifierKind::Isbn, 1, &s, &id).unwrap(),
            commit(&env, IdentifierKind::Barcode, 1, &s, &id).unwrap()
        );
    }

    #[test]
    fn every_identifier_kind_produces_a_distinct_commitment() {
        let env = Env::default();
        let s = salt(&env, 5);
        let id = ident(&env, b"9780262033848");

        let kinds = [
            IdentifierKind::Isbn,
            IdentifierKind::Barcode,
            IdentifierKind::StudentId,
            IdentifierKind::PatronId,
            IdentifierKind::DocumentHash,
            IdentifierKind::ReadingList,
        ];

        for (i, left) in kinds.iter().enumerate() {
            for right in kinds.iter().skip(i + 1) {
                assert_ne!(
                    commit(&env, *left, 1, &s, &id).unwrap(),
                    commit(&env, *right, 1, &s, &id).unwrap(),
                    "{left:?} and {right:?} share a commitment"
                );
            }
        }
    }

    #[test]
    fn the_same_student_id_at_two_institutions_is_uncorrelatable() {
        let env = Env::default();
        let id = ident(&env, b"00123456789012345");

        // Different institutions hold different salts, so a shared identifier
        // produces unrelated commitments — this is what stops cross-institution
        // tracking of the same person.
        assert_ne!(
            commit(&env, IdentifierKind::StudentId, 1, &salt(&env, 11), &id).unwrap(),
            commit(&env, IdentifierKind::StudentId, 1, &salt(&env, 22), &id).unwrap()
        );
    }

    #[test]
    fn rotating_the_salt_version_re_randomizes_every_commitment() {
        let env = Env::default();
        let s = salt(&env, 5);
        let id = ident(&env, b"9780262033848");

        let v1 = commit(&env, IdentifierKind::Isbn, 1, &s, &id).unwrap();
        let v2 = commit(&env, IdentifierKind::Isbn, 2, &s, &id).unwrap();

        assert_ne!(v1, v2);
        // A commitment from the old version does not verify under the new one,
        // so a stale record cannot silently keep working past the overlap.
        assert!(!verify(&env, IdentifierKind::Isbn, 2, &s, &id, &v1).unwrap());
    }

    #[test]
    fn distinct_identifiers_commit_distinctly_under_the_same_salt() {
        let env = Env::default();
        let s = salt(&env, 5);

        assert_ne!(
            commit(&env, IdentifierKind::Isbn, 1, &s, &ident(&env, b"9780262033848")).unwrap(),
            commit(&env, IdentifierKind::Isbn, 1, &s, &ident(&env, b"9780262033849")).unwrap()
        );
    }

    // ── verify ─────────────────────────────────────────────────────────────

    #[test]
    fn verify_accepts_the_commitment_it_produced() {
        let env = Env::default();
        let s = salt(&env, 8);
        let id = ident(&env, b"9780262033848");
        let c = commit(&env, IdentifierKind::Isbn, 1, &s, &id).unwrap();

        assert!(verify(&env, IdentifierKind::Isbn, 1, &s, &id, &c).unwrap());
    }

    #[test]
    fn verify_rejects_a_commitment_from_another_domain() {
        let env = Env::default();
        let s = salt(&env, 8);
        let id = ident(&env, b"9780262033848");
        let c = commit(&env, IdentifierKind::Isbn, 1, &s, &id).unwrap();

        assert!(!verify(&env, IdentifierKind::Barcode, 1, &s, &id, &c).unwrap());
        assert!(!verify(&env, IdentifierKind::Isbn, 2, &s, &id, &c).unwrap());
        assert!(!verify(&env, IdentifierKind::Isbn, 1, &salt(&env, 9), &id, &c).unwrap());
        assert!(!verify(
            &env,
            IdentifierKind::Isbn,
            1,
            &s,
            &ident(&env, b"9780262033849"),
            &c
        )
        .unwrap());
    }

    // ── Property test ──────────────────────────────────────────────────────

    /// xorshift64*, so every generated case replays from its seed.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
    }

    #[test]
    fn a_commitment_changes_whenever_any_domain_component_changes() {
        let env = Env::default();
        let kinds = [
            IdentifierKind::Isbn,
            IdentifierKind::Barcode,
            IdentifierKind::StudentId,
        ];

        for seed in [4u64, 44, 444] {
            let mut rng = Rng(seed);
            for _ in 0..200u32 {
                let kind_a = kinds[(rng.next() % 3) as usize];
                let kind_b = kinds[(rng.next() % 3) as usize];
                let version_a = (rng.next() % 4) as u32 + 1;
                let version_b = (rng.next() % 4) as u32 + 1;
                let salt_a = salt(&env, (rng.next() % 200) as u8 + 1);
                let salt_b = salt(&env, (rng.next() % 200) as u8 + 1);
                let id_a = ident(&env, &(rng.next() as u64).to_be_bytes());
                let id_b = ident(&env, &(rng.next() as u64).to_be_bytes());

                let a = commit(&env, kind_a, version_a, &salt_a, &id_a).unwrap();
                let b = commit(&env, kind_b, version_b, &salt_b, &id_b).unwrap();

                let same_domain = kind_a == kind_b
                    && version_a == version_b
                    && salt_a == salt_b
                    && id_a == id_b;

                if same_domain {
                    assert_eq!(a, b, "seed {seed}: identical inputs disagreed");
                } else {
                    assert_ne!(a, b, "seed {seed}: different inputs collided");
                }
            }
        }
    }
}
