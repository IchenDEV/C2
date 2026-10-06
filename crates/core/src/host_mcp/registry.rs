//! Per-session host MCP credentials: random bearer, hash-only storage, TTL with refresh-on-use.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const HOST_MCP_SERVER_NAME: &str = "codetwo";
pub const CREDENTIAL_TTL: Duration = Duration::from_secs(24 * 60 * 60);
pub const MAX_ACTIVE_CREDENTIALS: usize = 4096;

/// Capability granted to a issued bearer. Cross-session reads require an explicit extra capability.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HostMcpCapability {
    Capabilities,
    SessionList,
    SessionReadOwn,
    /// Reserved for future delegate/preview tools — not issued in this round.
    SessionReadCross,
}

impl HostMcpCapability {
    pub fn default_read_only_set() -> Vec<Self> {
        vec![Self::Capabilities, Self::SessionList, Self::SessionReadOwn]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Capabilities => "capabilities",
            Self::SessionList => "session_list",
            Self::SessionReadOwn => "session_read_own",
            Self::SessionReadCross => "session_read_cross",
        }
    }
}

#[derive(Debug, Clone)]
pub struct HostMcpScope {
    pub session_id: String,
    pub provider_id: String,
    pub capabilities: Vec<HostMcpCapability>,
}

#[derive(Debug, Clone)]
pub struct ResolvedHostMcpCredential {
    pub scope: HostMcpScope,
    pub credential_id: String,
}

struct StoredCredential {
    hash: [u8; 32],
    scope: HostMcpScope,
    expires_at: Instant,
}

#[derive(Default)]
pub struct HostMcpRegistry {
    by_id: HashMap<String, StoredCredential>,
    /// credential_id ordered for expiry sweep
    insertion_order: Vec<String>,
}

impl HostMcpRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    fn hash_token(token: &[u8]) -> [u8; 32] {
        let digest = Sha256::digest(token);
        let mut out = [0u8; 32];
        out.copy_from_slice(&digest);
        out
    }

    fn constant_time_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
        a.iter()
            .zip(b.iter())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
    }

    fn sweep_expired(&mut self, now: Instant) {
        self.by_id.retain(|_, entry| entry.expires_at > now);
        self.insertion_order
            .retain(|id| self.by_id.contains_key(id));
    }

    /// Issue a new bearer for `scope`. Returns `(credential_id, bearer_token)`; only the hash is stored.
    pub fn issue(&mut self, scope: HostMcpScope) -> Result<(String, String), String> {
        let now = Instant::now();
        self.sweep_expired(now);
        if self.by_id.len() >= MAX_ACTIVE_CREDENTIALS {
            return Err("host MCP credential limit reached".into());
        }
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        let mut token_bytes = [0u8; 32];
        token_bytes[..16].copy_from_slice(a.as_bytes());
        token_bytes[16..].copy_from_slice(b.as_bytes());
        let token = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            token_bytes,
        );
        let credential_id = uuid::Uuid::new_v4().to_string();
        let hash = Self::hash_token(token.as_bytes());
        self.by_id.insert(
            credential_id.clone(),
            StoredCredential {
                hash,
                scope,
                expires_at: now + CREDENTIAL_TTL,
            },
        );
        self.insertion_order.push(credential_id.clone());
        Ok((credential_id, token))
    }

    /// Validate bearer, refresh TTL on success.
    pub fn resolve(&mut self, bearer: &str) -> Option<ResolvedHostMcpCredential> {
        let now = Instant::now();
        self.sweep_expired(now);
        let hash = Self::hash_token(bearer.as_bytes());
        for (credential_id, stored) in &mut self.by_id {
            if Self::constant_time_eq(&stored.hash, &hash) {
                if stored.expires_at <= now {
                    return None;
                }
                stored.expires_at = now + CREDENTIAL_TTL;
                return Some(ResolvedHostMcpCredential {
                    scope: stored.scope.clone(),
                    credential_id: credential_id.clone(),
                });
            }
        }
        None
    }

    pub fn revoke_session(&mut self, session_id: &str) {
        self.by_id
            .retain(|_, entry| entry.scope.session_id != session_id);
        self.insertion_order
            .retain(|id| self.by_id.contains_key(id));
    }

    pub fn revoke_credential(&mut self, credential_id: &str) {
        self.by_id.remove(credential_id);
        self.insertion_order.retain(|id| id != credential_id);
    }

    #[cfg(test)]
    pub fn force_expire(&mut self, credential_id: &str) {
        if let Some(entry) = self.by_id.get_mut(credential_id) {
            entry.expires_at = Instant::now() - Duration::from_secs(1);
        }
    }

    #[cfg(test)]
    pub fn stored_hash_only(&self, credential_id: &str) -> bool {
        self.by_id.get(credential_id).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_scope(session: &str) -> HostMcpScope {
        HostMcpScope {
            session_id: session.into(),
            provider_id: "claude_code".into(),
            capabilities: HostMcpCapability::default_read_only_set(),
        }
    }

    #[test]
    fn stores_hash_only_and_resolves_with_refresh() {
        let mut registry = HostMcpRegistry::new();
        let (id, token) = registry.issue(sample_scope("s1")).unwrap();
        assert!(registry.stored_hash_only(&id));
        assert!(!token.is_empty());
        let resolved = registry.resolve(&token).expect("valid token");
        assert_eq!(resolved.scope.session_id, "s1");
        assert!(registry.resolve(&token).is_some());
        registry.revoke_credential(&id);
        assert!(registry.resolve(&token).is_none());
    }

    #[test]
    fn expires_and_revokes_by_session() {
        let mut registry = HostMcpRegistry::new();
        let (id, token) = registry.issue(sample_scope("s-expire")).unwrap();
        registry.force_expire(&id);
        assert!(registry.resolve(&token).is_none());
        let (_id2, token2) = registry.issue(sample_scope("s2")).unwrap();
        registry.revoke_session("s2");
        assert!(registry.resolve(&token2).is_none());
    }

    #[test]
    fn wrong_token_is_rejected() {
        let mut registry = HostMcpRegistry::new();
        registry.issue(sample_scope("s1")).unwrap();
        assert!(registry.resolve("not-a-valid-token").is_none());
    }

    #[test]
    fn hash_storage_does_not_retain_plain_bearer() {
        let mut registry = HostMcpRegistry::new();
        let (id, token) = registry.issue(sample_scope("s1")).unwrap();
        let stored_hash = registry.by_id.get(&id).map(|entry| entry.hash);
        assert!(stored_hash.is_some());
        assert_ne!(stored_hash.unwrap().as_slice(), token.as_bytes());
    }
}
