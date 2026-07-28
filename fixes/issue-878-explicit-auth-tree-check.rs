//! Fix for #878: assert an exact required-authorizer set for a
//! privileged entrypoint instead of a blanket mock-all-auths check,
//! so missing/incorrect require_auth calls are caught.
pub fn assert_exact_auth_tree(
    required: &[&str],
    observed_authorizers: &[&str],
) -> Result<(), &'static str> {
    if observed_authorizers.len() != required.len() {
        return Err("auth tree size mismatch");
    }
    for who in required {
        if !observed_authorizers.contains(who) {
            return Err("missing required authorizer");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrong_auth_is_rejected() {
        let result = assert_exact_auth_tree(&["admin"], &["attacker"]);
        assert!(result.is_err());
    }

    #[test]
    fn no_auth_is_rejected() {
        let result = assert_exact_auth_tree(&["admin"], &[]);
        assert!(result.is_err());
    }

    #[test]
    fn exact_required_auth_succeeds() {
        let result = assert_exact_auth_tree(&["admin"], &["admin"]);
        assert!(result.is_ok());
    }
}
