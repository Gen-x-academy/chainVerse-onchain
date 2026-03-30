use std::collections::HashMap;

/// Consent status for ABAC evaluation
#[derive(Debug, Clone, PartialEq)]
pub enum ConsentStatus {
    Active,
    Expired,
    Revoked,
    NotGranted,
}

/// Consent attribute for ABAC policies
#[derive(Debug, Clone)]
pub struct ConsentAttribute {
    pub subject: String,
    pub grantee: String,
    pub consent_type: ConsentType,
    pub status: ConsentStatus,
    pub granted_at: u64,
    pub expires_at: Option<u64>,
}

impl ConsentRecord {
    /// Get consent status at a given timestamp
    pub fn get_status_at(&self, now: u64) -> ConsentStatus {
        if self.revoked {
            ConsentStatus::Revoked
        } else if let Some(exp) = self.expires_at {
            if now < exp {
                ConsentStatus::Active
            } else {
                ConsentStatus::Expired
            }
        } else {
            ConsentStatus::Active
        }
    }

    /// Convert to consent attribute for ABAC evaluation
    pub fn to_attribute(&self, now: u64) -> ConsentAttribute {
        ConsentAttribute {
            subject: self.subject.clone(),
            grantee: self.grantee.clone(),
            consent_type: self.consent_type.clone(),
            status: self.get_status_at(now),
            granted_at: self.granted_at,
            expires_at: self.expires_at,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ConsentType {
    Treatment,
    Research,
    Sharing,
}

#[derive(Debug, Clone)]
pub struct ConsentRecord {
    pub subject: String,
    pub grantee: String,
    pub consent_type: ConsentType,
    pub granted_at: u64,
    pub expires_at: Option<u64>,
    pub revoked: bool,
}

#[derive(Default)]
pub struct ConsentManager {
    pub records: HashMap<String, ConsentRecord>,
}

impl ConsentManager {
    /// Grant consent using an externally supplied timestamp.
    ///
    /// Callers must provide the current time (`now`) rather than relying
    /// on `SystemTime`. In a Soroban contract context this is the ledger
    /// timestamp; in off-chain tooling it is `SystemTime::now()` converted
    /// to seconds since the UNIX epoch.
    pub fn grant(
        &mut self,
        id: &str,
        subject: &str,
        grantee: &str,
        ctype: ConsentType,
        now: u64,
        ttl_secs: Option<u64>,
    ) {
        let expires = ttl_secs.and_then(|t| now.checked_add(t));
        self.records.insert(
            id.to_string(),
            ConsentRecord {
                subject: subject.to_string(),
                grantee: grantee.to_string(),
                consent_type: ctype,
                granted_at: now,
                expires_at: expires,
                revoked: false,
            },
        );
    }

    pub fn revoke(&mut self, id: &str) {
        if let Some(r) = self.records.get_mut(id) {
            r.revoked = true;
        }
    }

    /// Check if consent is active at the given timestamp.
    ///
    /// Returns `false` when the record is missing, revoked, or expired
    /// relative to `now`.
    pub fn is_active(&self, id: &str, now: u64) -> bool {
        if let Some(r) = self.records.get(id) {
            if r.revoked {
                return false;
            }
            if let Some(exp) = r.expires_at {
                return now < exp;
            }
            return true;
        }
        false
    }

    /// Get consent attribute for ABAC evaluation
    pub fn get_consent_attribute(&self, id: &str, now: u64) -> Option<ConsentAttribute> {
        self.records.get(id).map(|record| record.to_attribute(now))
    }

    /// Check if consent exists and return its status
    pub fn get_consent_status(&self, id: &str, now: u64) -> ConsentStatus {
        match self.records.get(id) {
            Some(record) => record.get_status_at(now),
            None => ConsentStatus::NotGranted,
        }
    }

    /// Get all active consents for a specific grantee
    pub fn get_active_consents_for_grantee(
        &self,
        grantee: &str,
        now: u64,
    ) -> Vec<ConsentAttribute> {
        self.records
            .values()
            .filter(|record| record.grantee == grantee)
            .filter(|record| record.get_status_at(now) == ConsentStatus::Active)
            .map(|record| record.to_attribute(now))
            .collect()
    }

    /// Get all active consents for a specific subject
    pub fn get_active_consents_for_subject(
        &self,
        subject: &str,
        now: u64,
    ) -> Vec<ConsentAttribute> {
        self.records
            .values()
            .filter(|record| record.subject == subject)
            .filter(|record| record.get_status_at(now) == ConsentStatus::Active)
            .map(|record| record.to_attribute(now))
            .collect()
    }
}
