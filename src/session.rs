use axum::http::HeaderMap;
use serde::Deserialize;
use serde_json::value::RawValue;
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

const MAX_SESSIONS: usize = 64;
const MAX_HASHES: usize = 128;
#[derive(Deserialize)]
pub struct Envelope<'a> {
    #[serde(borrow)]
    pub messages: Vec<&'a RawValue>,
}
#[derive(Deserialize)]
struct Role<'a> {
    #[serde(borrow)]
    role: &'a str,
}
struct Entry {
    hashes: Vec<[u8; 32]>,
    id: String,
    last: Instant,
}
#[derive(Default)]
pub struct Sessions {
    entries: VecDeque<Entry>,
}
impl Sessions {
    pub fn resolve(&mut self, headers: &HeaderMap, envelope: &Envelope<'_>) -> String {
        for name in [
            "x-opencode-session",
            "x-workbuddy-session",
            "x-conversation-id",
            "x-session-id",
        ] {
            if let Some(value) = headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .filter(|s| !s.is_empty() && s.len() <= 256)
            {
                return hex(&Sha256::digest(format!("{name}:{value}")));
            }
        }
        let hashes: Vec<[u8; 32]> = envelope
            .messages
            .iter()
            .filter(|v| {
                serde_json::from_str::<Role<'_>>(v.get())
                    .is_ok_and(|r| !matches!(r.role, "system" | "developer"))
            })
            .take(MAX_HASHES)
            .map(|v| Sha256::digest(v.get().as_bytes()).into())
            .collect();
        if hashes.is_empty() {
            return crate::config::token();
        }
        self.entries
            .retain(|e| e.last.elapsed() < Duration::from_secs(24 * 3600));
        let best = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| hashes.starts_with(&e.hashes))
            .max_by_key(|(_, e)| e.hashes.len())
            .map(|(i, _)| i);
        if let Some(i) = best {
            let e = &mut self.entries[i];
            e.last = Instant::now();
            e.hashes = hashes;
            return e.id.clone();
        }
        let id = crate::config::token();
        if self.entries.len() >= MAX_SESSIONS {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry {
            hashes,
            id: id.clone(),
            last: Instant::now(),
        });
        id
    }
}
fn hex(data: &[u8]) -> String {
    data[..16].iter().map(|b| format!("{b:02x}")).collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn go_sessions_accept_multimodal_content_and_hash_media_changes() {
        let first: Envelope<'_> = serde_json::from_str(r#"{"messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"data:image/png;base64,AAAA"}}]}]}"#).unwrap();
        let changed: Envelope<'_> = serde_json::from_str(r#"{"messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"data:image/png;base64,BBBB"}}]}]}"#).unwrap();
        let mut sessions = Sessions::default();
        let headers = HeaderMap::new();
        let id = sessions.resolve(&headers, &first);
        assert_eq!(id, sessions.resolve(&headers, &first));
        assert_ne!(id, sessions.resolve(&headers, &changed));
    }
    #[test]
    fn continuation_isolation_and_explicit_id() {
        let first: Envelope<'_> =
            serde_json::from_str(r#"{"model":"m","messages":[{"role":"user","content":"a"}]}"#)
                .unwrap();
        let next:Envelope<'_>=serde_json::from_str(r#"{"model":"m","messages":[{"role":"system","content":"new"},{"role":"user","content":"a"},{"role":"assistant","tool_calls":[{"id":"x"}]}]}"#).unwrap();
        let other: Envelope<'_> =
            serde_json::from_str(r#"{"model":"m","messages":[{"role":"user","content":"b"}]}"#)
                .unwrap();
        let mut s = Sessions::default();
        let mut h = HeaderMap::new();
        let id = s.resolve(&h, &first);
        assert_eq!(id, s.resolve(&h, &next));
        assert_ne!(id, s.resolve(&h, &other));
        h.insert("x-opencode-session", "chat-id".parse().unwrap());
        assert_eq!(s.resolve(&h, &first), s.resolve(&h, &other));
    }
    #[test]
    fn cache_is_bounded() {
        let mut s = Sessions::default();
        for n in 0..128 {
            let json = format!(r#"{{"model":"m","messages":[{{"role":"user","content":"{n}"}}]}}"#);
            s.resolve(&HeaderMap::new(), &serde_json::from_str(&json).unwrap());
        }
        assert_eq!(s.entries.len(), 64);
    }
}
