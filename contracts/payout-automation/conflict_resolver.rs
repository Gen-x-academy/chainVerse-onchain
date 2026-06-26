//! Conflict detection and resolution for overlapping or contradictory policies.
//!
//! When multiple policies apply to the same request, this module determines
//! the final access decision using one of several configurable strategies.

use soroban_sdk::{contracttype, Env, Vec};

use crate::policy_dsl::{PolicyDefinition, PolicyEffect, PolicyId};

// ── Resolution Strategies ───────────────────────────────────────────────────

/// Strategy used to resolve conflicts when multiple policies match a request.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq, Copy)]
#[repr(u32)]
pub enum ResolutionStrategy {
    /// If any matching policy denies access, the overall result is Deny.
    DenyOverride = 1,
    /// If any matching policy permits access, the overall result is Permit.
    PermitOverride = 2,
    /// The first matching policy (ordered by priority) determines the outcome.
    FirstApplicable = 3,
}

// ── Conflict Record ─────────────────────────────────────────────────────────

/// Describes a detected conflict between two policies.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyConflict {
    pub policy_a: PolicyId,
    pub policy_b: PolicyId,
    pub resolved_effect: PolicyEffect,
}

// ── Resolution Result ───────────────────────────────────────────────────────

/// The final resolved outcome after applying the configured strategy to all
/// matching policies.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ResolutionResult {
    pub effect: PolicyEffect,
    pub conflicts: Vec<PolicyConflict>,
    /// Empty vec = no winner; single element = the winning policy.
    pub winning_policy: Vec<PolicyId>,
}

// ── Core Resolver ───────────────────────────────────────────────────────────

/// Detects conflicts among the provided policy effects and resolves them
/// according to the given strategy.
///
/// `matched` is a list of `(PolicyDefinition, PolicyEffect)` tuples — one
/// entry per policy that was evaluated as applicable. The `PolicyEffect` is
/// the outcome from the engine (Permit / Deny) after evaluating the rule
/// tree against the request context.
pub fn resolve(
    env: &Env,
    strategy: ResolutionStrategy,
    matched: &Vec<(PolicyDefinition, PolicyEffect)>,
) -> ResolutionResult {
    if matched.is_empty() {
        return ResolutionResult {
            effect: PolicyEffect::Deny,
            conflicts: Vec::new(env),
            winning_policy: Vec::new(env),
        };
    }

    let mut conflicts: Vec<PolicyConflict> = Vec::new(env);

    // Detect pair-wise conflicts (different effects from different policies)
    detect_conflicts(env, matched, &mut conflicts);

    let (effect, winner) = match strategy {
        ResolutionStrategy::DenyOverride => resolve_deny_override(matched),
        ResolutionStrategy::PermitOverride => resolve_permit_override(matched),
        ResolutionStrategy::FirstApplicable => resolve_first_applicable(matched),
    };

    // Tag each detected conflict with the resolved effect
    let mut tagged_conflicts: Vec<PolicyConflict> = Vec::new(env);
    for i in 0..conflicts.len() {
        let mut c = conflicts.get(i).unwrap();
        c.resolved_effect = effect;
        tagged_conflicts.push_back(c);
    }

    let mut winning_policy: Vec<PolicyId> = Vec::new(env);
    if let Some(w) = winner {
        winning_policy.push_back(w);
    }

    ResolutionResult {
        effect,
        conflicts: tagged_conflicts,
        winning_policy,
    }
}

// ── Internal helpers ────────────────────────────────────────────────────────

fn detect_conflicts(
    env: &Env,
    matched: &Vec<(PolicyDefinition, PolicyEffect)>,
    out: &mut Vec<PolicyConflict>,
) {
    let len = matched.len();
    for i in 0..len {
        let (def_a, eff_a) = matched.get(i).unwrap();
        for j in (i + 1)..len {
            let (def_b, eff_b) = matched.get(j).unwrap();
            if eff_a != eff_b {
                out.push_back(PolicyConflict {
                    policy_a: def_a.id.clone(),
                    policy_b: def_b.id.clone(),
                    // Placeholder; will be overwritten by the caller with the
                    // final resolved effect.
                    resolved_effect: PolicyEffect::Deny,
                });
            }
        }
    }
    let _ = env; // keep signature consistent
}

/// Deny wins if any matched policy evaluates to Deny.
fn resolve_deny_override(
    matched: &Vec<(PolicyDefinition, PolicyEffect)>,
) -> (PolicyEffect, Option<PolicyId>) {
    for i in 0..matched.len() {
        let (def, eff) = matched.get(i).unwrap();
        if eff == PolicyEffect::Deny {
            return (PolicyEffect::Deny, Some(def.id.clone()));
        }
    }
    // All permit
    let (first_def, _) = matched.get(0).unwrap();
    (PolicyEffect::Permit, Some(first_def.id.clone()))
}

/// Permit wins if any matched policy evaluates to Permit.
fn resolve_permit_override(
    matched: &Vec<(PolicyDefinition, PolicyEffect)>,
) -> (PolicyEffect, Option<PolicyId>) {
    for i in 0..matched.len() {
        let (def, eff) = matched.get(i).unwrap();
        if eff == PolicyEffect::Permit {
            return (PolicyEffect::Permit, Some(def.id.clone()));
        }
    }
    // All deny
    let (first_def, _) = matched.get(0).unwrap();
    (PolicyEffect::Deny, Some(first_def.id.clone()))
}

/// The first policy in priority order determines the outcome.
/// Assumes the input is already sorted by priority (lowest number = highest
/// priority). If not pre-sorted, the caller is responsible for ordering.
fn resolve_first_applicable(
    matched: &Vec<(PolicyDefinition, PolicyEffect)>,
) -> (PolicyEffect, Option<PolicyId>) {
    let (first_def, first_eff) = matched.get(0).unwrap();
    (first_eff, Some(first_def.id.clone()))
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy_dsl::{PolicyDefinition, PolicyEffect, PolicyId, PolicyRule};
    use soroban_sdk::Env;

    fn make_policy(env: &Env, name: &str, effect: PolicyEffect, priority: u32) -> PolicyDefinition {
        PolicyDefinition {
            id: PolicyId {
                name: soroban_sdk::String::from_str(env, name),
                version: 1,
            },
            description: soroban_sdk::String::from_str(env, "test"),
            rule: PolicyRule::Allow,
            effect,
            priority,
            enabled: true,
        }
    }

    #[test]
    fn deny_override_denies_when_any_deny() {
        let env = Env::default();
        let p1 = make_policy(&env, "p1", PolicyEffect::Permit, 1);
        let p2 = make_policy(&env, "p2", PolicyEffect::Deny, 2);

        let mut matched = Vec::new(&env);
        matched.push_back((p1, PolicyEffect::Permit));
        matched.push_back((p2, PolicyEffect::Deny));

        let result = resolve(&env, ResolutionStrategy::DenyOverride, &matched);
        assert_eq!(result.effect, PolicyEffect::Deny);
        assert_eq!(result.conflicts.len(), 1);
    }

    #[test]
    fn deny_override_permits_when_all_permit() {
        let env = Env::default();
        let p1 = make_policy(&env, "p1", PolicyEffect::Permit, 1);
        let p2 = make_policy(&env, "p2", PolicyEffect::Permit, 2);

        let mut matched = Vec::new(&env);
        matched.push_back((p1, PolicyEffect::Permit));
        matched.push_back((p2, PolicyEffect::Permit));

        let result = resolve(&env, ResolutionStrategy::DenyOverride, &matched);
        assert_eq!(result.effect, PolicyEffect::Permit);
        assert!(result.conflicts.is_empty());
    }

    #[test]
    fn permit_override_permits_when_any_permit() {
        let env = Env::default();
        let p1 = make_policy(&env, "p1", PolicyEffect::Deny, 1);
        let p2 = make_policy(&env, "p2", PolicyEffect::Permit, 2);

        let mut matched = Vec::new(&env);
        matched.push_back((p1, PolicyEffect::Deny));
        matched.push_back((p2, PolicyEffect::Permit));

        let result = resolve(&env, ResolutionStrategy::PermitOverride, &matched);
        assert_eq!(result.effect, PolicyEffect::Permit);
        assert_eq!(result.conflicts.len(), 1);
    }

    #[test]
    fn first_applicable_uses_first_entry() {
        let env = Env::default();
        let p1 = make_policy(&env, "high_prio", PolicyEffect::Permit, 1);
        let p2 = make_policy(&env, "low_prio", PolicyEffect::Deny, 10);

        let mut matched = Vec::new(&env);
        matched.push_back((p1, PolicyEffect::Permit));
        matched.push_back((p2, PolicyEffect::Deny));

        let result = resolve(&env, ResolutionStrategy::FirstApplicable, &matched);
        assert_eq!(result.effect, PolicyEffect::Permit);
    }

    #[test]
    fn empty_matched_defaults_to_deny() {
        let env = Env::default();
        let matched: Vec<(PolicyDefinition, PolicyEffect)> = Vec::new(&env);
        let result = resolve(&env, ResolutionStrategy::DenyOverride, &matched);
        assert_eq!(result.effect, PolicyEffect::Deny);
        assert!(result.winning_policy.is_empty());
    }

    #[test]
    fn multiple_conflicts_detected() {
        let env = Env::default();
        let p1 = make_policy(&env, "p1", PolicyEffect::Permit, 1);
        let p2 = make_policy(&env, "p2", PolicyEffect::Deny, 2);
        let p3 = make_policy(&env, "p3", PolicyEffect::Permit, 3);

        let mut matched = Vec::new(&env);
        matched.push_back((p1, PolicyEffect::Permit));
        matched.push_back((p2, PolicyEffect::Deny));
        matched.push_back((p3, PolicyEffect::Permit));

        let result = resolve(&env, ResolutionStrategy::DenyOverride, &matched);
        // p1 vs p2 conflict, p2 vs p3 conflict => 2 conflicts
        assert_eq!(result.conflicts.len(), 2);
        assert_eq!(result.effect, PolicyEffect::Deny);
    }
}
