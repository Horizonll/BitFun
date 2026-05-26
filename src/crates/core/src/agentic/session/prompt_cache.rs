use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const PROMPT_CACHE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptCachePolicy {
    pub cache_ttl: Option<Duration>,
    pub persistence_ttl: Option<Duration>,
}

impl Default for PromptCachePolicy {
    fn default() -> Self {
        Self {
            cache_ttl: None,
            persistence_ttl: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemPromptCacheIdentity {
    pub agent_id: String,
    pub prompt_identity: String,
}

impl SystemPromptCacheIdentity {
    pub fn new(agent_id: impl Into<String>, prompt_identity: impl Into<String>) -> Self {
        Self {
            agent_id: agent_id.into(),
            prompt_identity: prompt_identity.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedPromptText {
    pub content: String,
    pub created_at_ms: u64,
}

impl CachedPromptText {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            created_at_ms: current_time_ms(),
        }
    }

    pub fn is_expired(&self, ttl: Option<Duration>, now_ms: u64) -> bool {
        ttl.is_some_and(|ttl| {
            let ttl_ms = ttl.as_millis().try_into().unwrap_or(u64::MAX);
            now_ms.saturating_sub(self.created_at_ms) >= ttl_ms
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedSystemPrompt {
    #[serde(flatten)]
    pub text: CachedPromptText,
    pub identity: SystemPromptCacheIdentity,
}

impl CachedSystemPrompt {
    pub fn new(identity: SystemPromptCacheIdentity, content: impl Into<String>) -> Self {
        Self {
            text: CachedPromptText::new(content),
            identity,
        }
    }

    pub fn is_usable(
        &self,
        identity: &SystemPromptCacheIdentity,
        ttl: Option<Duration>,
        now_ms: u64,
    ) -> bool {
        self.identity == *identity && !self.text.is_expired(ttl, now_ms)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionPromptCache {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<CachedSystemPrompt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_context: Option<CachedPromptText>,
}

impl SessionPromptCache {
    pub fn apply_persistence_ttl(&mut self, ttl: Option<Duration>) -> bool {
        let now_ms = current_time_ms();
        let mut changed = false;

        if self
            .system_prompt
            .as_ref()
            .is_some_and(|entry| entry.text.is_expired(ttl, now_ms))
        {
            self.system_prompt = None;
            changed = true;
        }

        if self
            .user_context
            .as_ref()
            .is_some_and(|entry| entry.is_expired(ttl, now_ms))
        {
            self.user_context = None;
            changed = true;
        }

        changed
    }

    pub fn is_empty(&self) -> bool {
        self.system_prompt.is_none() && self.user_context.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptCacheScope {
    SystemPrompt,
    UserContext,
    All,
}

impl PromptCacheScope {
    fn clears_system_prompt(self) -> bool {
        matches!(self, Self::SystemPrompt | Self::All)
    }

    fn clears_user_context(self) -> bool {
        matches!(self, Self::UserContext | Self::All)
    }
}

pub struct SessionPromptCacheStore {
    session_caches: Arc<DashMap<String, SessionPromptCache>>,
}

pub enum PromptCacheLookup {
    Hit(String),
    Miss,
    Expired,
}

impl Default for SessionPromptCacheStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionPromptCacheStore {
    pub fn new() -> Self {
        Self {
            session_caches: Arc::new(DashMap::new()),
        }
    }

    pub fn create_session(&self, session_id: &str) {
        self.session_caches
            .entry(session_id.to_string())
            .or_default();
    }

    pub fn has_session(&self, session_id: &str) -> bool {
        self.session_caches.contains_key(session_id)
    }

    pub fn replace_cache(&self, session_id: &str, cache: SessionPromptCache) {
        self.session_caches.insert(session_id.to_string(), cache);
    }

    pub fn get_cache(&self, session_id: &str) -> Option<SessionPromptCache> {
        self.session_caches
            .get(session_id)
            .map(|cache| cache.clone())
    }

    pub fn lookup_system_prompt(
        &self,
        session_id: &str,
        identity: &SystemPromptCacheIdentity,
        ttl: Option<Duration>,
    ) -> PromptCacheLookup {
        let now_ms = current_time_ms();
        let cached_entry = self
            .session_caches
            .get(session_id)
            .and_then(|cache| cache.system_prompt.clone());

        match cached_entry {
            Some(entry) if entry.is_usable(identity, ttl, now_ms) => {
                PromptCacheLookup::Hit(entry.text.content)
            }
            Some(entry) if entry.text.is_expired(ttl, now_ms) => {
                self.invalidate(session_id, PromptCacheScope::SystemPrompt);
                PromptCacheLookup::Expired
            }
            _ => PromptCacheLookup::Miss,
        }
    }

    pub fn lookup_user_context(
        &self,
        session_id: &str,
        ttl: Option<Duration>,
    ) -> PromptCacheLookup {
        let now_ms = current_time_ms();
        let cached_entry = self
            .session_caches
            .get(session_id)
            .and_then(|cache| cache.user_context.clone());

        match cached_entry {
            Some(entry) if !entry.is_expired(ttl, now_ms) => PromptCacheLookup::Hit(entry.content),
            Some(_) => {
                self.invalidate(session_id, PromptCacheScope::UserContext);
                PromptCacheLookup::Expired
            }
            None => PromptCacheLookup::Miss,
        }
    }

    pub fn set_system_prompt(&self, session_id: &str, entry: CachedSystemPrompt) {
        if let Some(mut cache) = self.session_caches.get_mut(session_id) {
            cache.system_prompt = Some(entry);
        } else {
            self.session_caches.insert(
                session_id.to_string(),
                SessionPromptCache {
                    system_prompt: Some(entry),
                    user_context: None,
                },
            );
        }
    }

    pub fn set_user_context(&self, session_id: &str, entry: CachedPromptText) {
        if let Some(mut cache) = self.session_caches.get_mut(session_id) {
            cache.user_context = Some(entry);
        } else {
            self.session_caches.insert(
                session_id.to_string(),
                SessionPromptCache {
                    system_prompt: None,
                    user_context: Some(entry),
                },
            );
        }
    }

    pub fn invalidate(&self, session_id: &str, scope: PromptCacheScope) -> bool {
        let Some(mut cache) = self.session_caches.get_mut(session_id) else {
            return false;
        };

        let mut changed = false;
        if scope.clears_system_prompt() && cache.system_prompt.take().is_some() {
            changed = true;
        }
        if scope.clears_user_context() && cache.user_context.take().is_some() {
            changed = true;
        }
        changed
    }

    pub fn delete_session(&self, session_id: &str) {
        self.session_caches.remove(session_id);
    }
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::{
        CachedPromptText, CachedSystemPrompt, PromptCacheLookup, PromptCacheScope,
        SessionPromptCacheStore, SystemPromptCacheIdentity,
    };
    use std::time::Duration;

    #[test]
    fn system_prompt_cache_requires_matching_identity() {
        let store = SessionPromptCacheStore::new();
        store.create_session("session-1");
        store.set_system_prompt(
            "session-1",
            CachedSystemPrompt::new(
                SystemPromptCacheIdentity::new("agentic", "template:agentic_mode"),
                "prompt-a",
            ),
        );

        assert_eq!(
            match store.lookup_system_prompt(
                "session-1",
                &SystemPromptCacheIdentity::new("agentic", "template:agentic_mode"),
                None,
            ) {
                PromptCacheLookup::Hit(value) => Some(value),
                _ => None,
            },
            Some("prompt-a".to_string())
        );
        assert!(matches!(
            store.lookup_system_prompt(
                "session-1",
                &SystemPromptCacheIdentity::new("debug", "template:debug_mode"),
                None,
            ),
            PromptCacheLookup::Miss
        ));
    }

    #[test]
    fn expired_user_context_is_evicted_on_read() {
        let store = SessionPromptCacheStore::new();
        store.create_session("session-1");
        store.set_user_context("session-1", CachedPromptText::new("stale context"));

        assert!(matches!(
            store.lookup_user_context("session-1", Some(Duration::from_millis(0))),
            PromptCacheLookup::Expired
        ));
        assert!(store
            .get_cache("session-1")
            .expect("session cache")
            .user_context
            .is_none());
    }

    #[test]
    fn invalidate_scope_can_clear_all_cached_prompt_parts() {
        let store = SessionPromptCacheStore::new();
        store.create_session("session-1");
        store.set_system_prompt(
            "session-1",
            CachedSystemPrompt::new(
                SystemPromptCacheIdentity::new("agentic", "template:agentic_mode"),
                "prompt-a",
            ),
        );
        store.set_user_context("session-1", CachedPromptText::new("context"));

        assert!(store.invalidate("session-1", PromptCacheScope::All));

        let cache = store.get_cache("session-1").expect("session cache");
        assert!(cache.system_prompt.is_none());
        assert!(cache.user_context.is_none());
    }
}
