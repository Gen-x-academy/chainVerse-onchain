//! Fix for #875: unify the public claim ABI so it verifies a canonical
//! signed payload (course, amount, nonce, expiry) instead of trusting
//! a bare user argument.
pub struct ClaimPayload {
    pub course: u64,
    pub amount: u64,
    pub nonce: u64,
    pub expiry: u64,
}
/// Stand-in for the real signature check: a canonical payload must
/// match the signer-provided digest before any payout happens.
fn canonical_digest(p: &ClaimPayload) -> String {
    format!("{}:{}:{}:{}", p.course, p.amount, p.nonce, p.expiry)
}
pub fn verify_claim(
    payload: &ClaimPayload,
    signed_digest: &str,
    now: u64,
    used_nonces: &[u64],
) -> Result<u64, &'static str> {
    if payload.expiry < now {
        return Err("claim expired");
    }
    if used_nonces.contains(&payload.nonce) {
        return Err("nonce already consumed");
    }
    if canonical_digest(payload) != signed_digest {
        return Err("signature payload mismatch");
    }
    Ok(payload.amount)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_signed_claim_pays_signed_amount() {
        let payload = ClaimPayload { course: 1, amount: 500, nonce: 7, expiry: 1000 };
        let digest = canonical_digest(&payload);
        assert_eq!(verify_claim(&payload, &digest, 100, &[]), Ok(500));
    }

    #[test]
    fn reused_nonce_is_rejected() {
        let payload = ClaimPayload { course: 1, amount: 500, nonce: 7, expiry: 1000 };
        let digest = canonical_digest(&payload);
        assert!(verify_claim(&payload, &digest, 100, &[7]).is_err());
    }
}
