//! 为 OpenCode Go 的 Chat 请求推导稳定会话标识。
//!
//! 调用方是 `proxy::relay_to`：只有目标被识别为 OpenCode Go 时，才把请求
//! `messages` 交给这里。该状态归 `Bridge.sessions: Mutex<Sessions>` 所有；同一
//! `proxy-io` Tokio runtime 上的请求通过互斥锁串行访问，不跨进程持久化。
//!
//! 优先读取显式会话头，其次查找消息前缀哈希，最后生成随机令牌。缓存最多保留 64
//! 个条目，在非空消息查找时清理闲置满 24 小时的条目。每个前缀快照最多包含 128 个
//! 可解析且非 system/developer 的消息哈希；缓存没有跨重启恢复能力。
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
/// Chat Completions 请求里 `messages` 的借用视图。
///
/// `RawValue` 保留每个消息原始 JSON，避免仅为了计算会话哈希而构造完整聊天对象。
/// `Envelope<'a>` 的生命周期直接借自 `relay_to` 中原始 Chat 请求 body；它不能比该
/// body buffer 活得更久；借用的界限来自原始字节，而不是结构体变量的名字或作用域。
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
    /// 已观察到的非 system/developer 消息哈希，按请求顺序排列。
    hashes: Vec<[u8; 32]>,
    id: String,
    last: Instant,
}
#[derive(Default)]
pub struct Sessions {
    entries: VecDeque<Entry>,
}
impl Sessions {
    /// 从请求头或消息前缀解析 OpenCode Go 会话 ID。
    ///
    /// 输入是请求头和已经解析出的消息借用视图。对四个候选头名依次调用
    /// `HeaderMap::get`：取第一个能转为文本、非空且不超过 256 字节的值，返回该
    /// 32 字符 SHA-256 前缀。这里不拒绝同名头的其他值；本函数也完全不检查
    /// `x-conversation-message-id`，那个头只由统计身份逻辑使用。
    ///
    /// 没有可用头时，按原顺序收集最多 128 个能解析出 `role` 且不是
    /// system/developer 的消息哈希。这个上限约束保留的非 system/developer 哈希数；
    /// 为了找到它们，迭代器仍可能扫描更多原始消息。空集合和首次出现的新哈希都返回
    /// `config::token()`（48 字符）；命中已有前缀则返回该条目先前保存的随机 ID。
    /// 命中后会把该条目的哈希更新为当前请求，并刷新其时间。
    ///
    /// 该函数只更新进程内缓存，不返回错误，不修改请求。只有在消息哈希集合非空并开始
    /// 查找时，才会清理超过 24 小时的条目；命中后会用当前请求的哈希列表覆盖旧列表。
    /// 插入新条目时若已有 64 条，只弹出最前面的一个。跨重启恢复没有保证。
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
