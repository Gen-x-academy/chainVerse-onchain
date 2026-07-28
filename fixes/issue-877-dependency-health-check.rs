//! Fix for #877: validate configured dependency addresses implement
//! the expected interface/version before accepting them, and expose a
//! read-only health summary.
use std::collections::HashMap;

pub struct DependencyHealth {
    pub healthy: bool,
    pub reason: Option<&'static str>,
}

pub struct CoreConfig {
    dependency_versions: HashMap<String, u32>,
    expected_version: u32,
}
impl CoreConfig {
    pub fn new(expected_version: u32) -> Self {
        Self { dependency_versions: HashMap::new(), expected_version }
    }

    pub fn set_dependency(&mut self, name: &str, version: u32) -> Result<(), &'static str> {
        if version != self.expected_version {
            return Err("dependency ABI/version mismatch");
        }
        self.dependency_versions.insert(name.to_string(), version);
        Ok(())
    }

    pub fn health(&self, name: &str) -> DependencyHealth {
        match self.dependency_versions.get(name) {
            Some(v) if *v == self.expected_version => DependencyHealth { healthy: true, reason: None },
            Some(_) => DependencyHealth { healthy: false, reason: Some("version mismatch") },
            None => DependencyHealth { healthy: false, reason: Some("not configured") },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incompatible_dependency_is_rejected_at_config_time() {
        let mut cfg = CoreConfig::new(2);
        assert!(cfg.set_dependency("oracle", 1).is_err());
        cfg.set_dependency("oracle", 2).unwrap();
        assert!(cfg.health("oracle").healthy);
    }
}
