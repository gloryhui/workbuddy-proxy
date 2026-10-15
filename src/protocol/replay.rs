//! Opaque native assistant state, bounded and memory-only. SQLite and logs must
//! never receive this state. A pending item is committed only at valid EOF.
use super::{
    Error,
    encode::{ReplayTurn, ResolvedHistory},
    request::{Message, Part, Role, TOOL_LIMIT},
};
use rand::RngCore;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, VecDeque},
    io,
    sync::Arc,
    time::{Duration, Instant},
};

const TTL: Duration = Duration::from_secs(30 * 60);
const MAX_ITEMS: usize = 512;
const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_RESPONSE: usize = 8 * 1024 * 1024;
const MAX_ITEM: usize = 1024 * 1024;

/// Generated per effective route generation, also binding protocol, model,
/// endpoint, key and settings. A changed route cannot resurrect old signatures.
pub type RouteKey = [u8; 32];
pub fn tool_id() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    let mut out = String::from("wbpt_");
    use std::fmt::Write;
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

pub struct Pending {
    route: RouteKey,
    binding: [u8; 32],
    turn: Arc<ReplayTurn>,
    bytes: usize,
}

fn expired() -> Error {
    Error {
        code: "conversation_state_expired",
        param: None,
        status: 409,
    }
}
fn invalid() -> Error {
    Error::input("conversation_state_mismatch", "messages")
}

/// Count serialized size without creating another copy of opaque strings.
#[derive(Default)]
struct Counter(usize);
impl io::Write for Counter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0 = self.0.saturating_add(buf.len());
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn json_bytes(value: &Value) -> usize {
    let mut count = Counter::default();
    if serde_json::to_writer(&mut count, value).is_err() {
        return usize::MAX;
    }
    count.0
}

/// Conservative retained allocation charge, including Value nodes, container
/// spare capacity and tree node overhead. Raw JSON byte length alone seriously
/// undercounts small-object-heavy trees.
fn allocation(value: &Value) -> usize {
    match value {
        Value::String(s) => s.capacity(),
        Value::Array(v) => {
            v.capacity() * std::mem::size_of::<Value>() + v.iter().map(allocation).sum::<usize>()
        }
        Value::Object(m) => {
            (m.len() + 1)
                * (std::mem::size_of::<(String, Value)>() + 12 * std::mem::size_of::<usize>())
                * 2
                + m.iter()
                    .map(|(k, v)| k.capacity() + allocation(v))
                    .sum::<usize>()
        }
        _ => 0,
    }
}

impl Pending {
    pub fn new(
        route: RouteKey,
        prefix: &[Message],
        assistant: &Message,
        turn: ReplayTurn,
    ) -> Result<Self, Error> {
        if assistant.role != Role::Assistant
            || assistant.calls.is_empty()
            || assistant.calls.len() > TOOL_LIMIT
            || assistant.calls.len() != turn.calls.len()
        {
            return Err(Error::upstream("invalid_replay_state"));
        }
        for call in &assistant.calls {
            if !call.id.starts_with("wbpt_")
                || call.id.len() != 37
                || !call.id[5..].bytes().all(|b| b.is_ascii_hexdigit())
                || turn
                    .calls
                    .get(&call.id)
                    .is_none_or(|native| native.name != call.name)
            {
                return Err(Error::upstream("invalid_replay_state"));
            }
        }
        if json_bytes(&turn.assistant) > MAX_RESPONSE {
            return Err(Error::upstream("conversation_state_too_large"));
        }
        let items = turn
            .assistant
            .as_array()
            .or_else(|| turn.assistant.get("parts").and_then(Value::as_array))
            .ok_or_else(|| Error::upstream("invalid_replay_state"))?;
        if items.iter().any(|v| json_bytes(v) > MAX_ITEM) {
            return Err(Error::upstream("conversation_state_too_large"));
        }
        let bytes = allocation(&turn.assistant)
            + std::mem::size_of::<ReplayTurn>()
            + turn
                .calls
                .iter()
                .map(|(id, c)| id.capacity() * 2 + c.id.capacity() + c.name.capacity() + 512)
                .sum::<usize>();
        if bytes > MAX_BYTES {
            return Err(Error::upstream("conversation_state_too_large"));
        }
        let binding = binding(prefix, assistant);
        Ok(Self {
            route,
            binding,
            turn: Arc::new(turn),
            bytes,
        })
    }
}

struct Entry {
    pending: Pending,
    created: Instant,
}
#[derive(Default)]
pub struct Cache {
    by_id: BTreeMap<String, Arc<Entry>>,
    oldest: VecDeque<Arc<Entry>>,
    bytes: usize,
}
impl Cache {
    fn evict_oldest(&mut self) {
        if let Some(entry) = self.oldest.pop_front() {
            self.bytes = self.bytes.saturating_sub(entry.pending.bytes);
            for id in entry.pending.turn.calls.keys() {
                self.by_id.remove(id);
            }
        }
    }
    fn expire(&mut self, now: Instant) {
        while self
            .oldest
            .front()
            .is_some_and(|e| now.saturating_duration_since(e.created) >= TTL)
        {
            self.evict_oldest();
        }
    }
    pub fn commit(&mut self, pending: Pending, now: Instant) -> Result<(), Error> {
        self.expire(now);
        if pending
            .turn
            .calls
            .keys()
            .any(|id| self.by_id.contains_key(id))
        {
            return Err(Error::upstream("duplicate_tool_id"));
        }
        while self.bytes + pending.bytes > MAX_BYTES
            || self.by_id.len() + pending.turn.calls.len() > MAX_ITEMS
        {
            self.evict_oldest();
        }
        self.bytes += pending.bytes;
        let entry = Arc::new(Entry {
            pending,
            created: now,
        });
        for id in entry.pending.turn.calls.keys() {
            self.by_id.insert(id.clone(), entry.clone());
        }
        self.oldest.push_back(entry);
        Ok(())
    }
    pub fn resolve(
        &mut self,
        route: &RouteKey,
        messages: &[Message],
        now: Instant,
    ) -> Result<ResolvedHistory, Error> {
        self.expire(now);
        let mut out = ResolvedHistory::new();
        for (i, m) in messages.iter().enumerate() {
            if m.role != Role::Assistant || !m.calls.iter().any(|c| c.id.starts_with("wbpt_")) {
                continue;
            }
            let first = m.calls.first().ok_or_else(invalid)?;
            let entry = self.by_id.get(&first.id).ok_or_else(expired)?;
            if &entry.pending.route != route {
                return Err(expired());
            }
            for call in &m.calls {
                let other = self.by_id.get(&call.id).ok_or_else(expired)?;
                if !Arc::ptr_eq(entry, other) {
                    return Err(invalid());
                }
            }
            if m.calls.len() != entry.pending.turn.calls.len()
                || binding(&messages[..i], m) != entry.pending.binding
            {
                return Err(invalid());
            }
            out.insert(i, entry.pending.turn.clone());
        }
        Ok(out)
    }
}

fn field(hash: &mut Sha256, s: &str) {
    hash.update((s.len() as u64).to_le_bytes());
    hash.update(s.as_bytes());
}
fn hash_message(hash: &mut Sha256, m: &Message) {
    hash.update([match m.role {
        Role::System => 0,
        Role::Developer => 1,
        Role::User => 2,
        Role::Assistant => 3,
        Role::Tool => 4,
    }]);
    hash.update((m.parts.len() as u64).to_le_bytes());
    for part in &m.parts {
        match part {
            Part::Text(s) => {
                hash.update([0]);
                field(hash, s);
            }
            Part::ImageData { mime, data, detail } => {
                hash.update([1]);
                field(hash, mime);
                field(hash, data);
                field(hash, detail);
            }
            Part::ImageUrl { url, detail } => {
                hash.update([2]);
                field(hash, url);
                field(hash, detail);
            }
        }
    }
    hash.update((m.calls.len() as u64).to_le_bytes());
    for call in &m.calls {
        field(hash, &call.id);
        field(hash, &call.name);
        // BTree-backed serde_json maps canonicalize object key order. Do not
        // bind insignificant argument whitespace to WorkBuddy's serializer.
        field(
            hash,
            &serde_json::to_string(&call.arguments).expect("JSON Value serialization"),
        );
    }
    field(hash, m.tool_call_id.as_deref().unwrap_or(""));
}
fn binding(prefix: &[Message], assistant: &Message) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"workbuddy-native-replay-v1");
    hash.update((prefix.len() as u64).to_le_bytes());
    for m in prefix {
        hash_message(&mut hash, m);
    }
    hash_message(&mut hash, assistant);
    hash.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::super::{
        encode::NativeCall,
        request::{ChatRequest, ToolCall},
    };
    use super::*;
    use serde_json::json;
    fn fixture() -> (Vec<Message>, Message, ReplayTurn) {
        let r =
            ChatRequest::parse(br#"{"model":"m","messages":[{"role":"user","content":"hello"}]}"#)
                .unwrap();
        let id = tool_id();
        let assistant = Message {
            role: Role::Assistant,
            parts: vec![Part::Text("checking".into())],
            calls: vec![ToolCall {
                id: id.clone(),
                name: "f".into(),
                arguments: json!({"q":1}),
                arguments_text: "{\"q\":1}".into(),
            }],
            tool_call_id: None,
        };
        let turn = ReplayTurn {
            assistant: json!([{ "type":"thinking","signature":"opaque" },{"type":"tool_use","id":"native","name":"f","input":{"q":1}}]),
            calls: BTreeMap::from([(
                id,
                NativeCall {
                    id: "native".into(),
                    name: "f".into(),
                },
            )]),
        };
        (r.messages, assistant, turn)
    }
    #[test]
    fn pending_cancel_never_publishes_state_and_retries_can_reuse_committed_state() {
        let (mut prefix, assistant, turn) = fixture();
        let now = Instant::now();
        let route = [1; 32];
        let mut cache = Cache::default();
        let pending = Pending::new(route, &prefix, &assistant, turn).unwrap();
        prefix.push(assistant);
        assert_eq!(cache.resolve(&route, &prefix, now).unwrap_err().status, 409);
        cache.commit(pending, now).unwrap();
        assert_eq!(cache.resolve(&route, &prefix, now).unwrap().len(), 1);
        assert_eq!(cache.resolve(&route, &prefix, now).unwrap().len(), 1);
        assert_eq!(
            cache.resolve(&[2; 32], &prefix, now).unwrap_err().status,
            409
        );
        prefix[0].parts = vec![Part::Text("tampered".into())];
        assert_eq!(cache.resolve(&route, &prefix, now).unwrap_err().status, 400);
    }
    #[test]
    fn expiry_and_capacity_eviction_are_bounded_and_discard_whole_groups() {
        let route = [1; 32];
        let now = Instant::now();
        let mut cache = Cache::default();
        let mut oldest = None;
        for _ in 0..MAX_ITEMS + 1 {
            let (mut prefix, assistant, turn) = fixture();
            let pending = Pending::new(route, &prefix, &assistant, turn).unwrap();
            prefix.push(assistant);
            if oldest.is_none() {
                oldest = Some(prefix);
            }
            cache.commit(pending, now).unwrap();
        }
        assert_eq!(cache.by_id.len(), MAX_ITEMS);
        assert!(cache.bytes <= MAX_BYTES);
        assert_eq!(
            cache
                .resolve(&route, &oldest.unwrap(), now)
                .unwrap_err()
                .status,
            409
        );
        cache.expire(now + TTL);
        assert!(cache.by_id.is_empty());
        assert_eq!(cache.bytes, 0);
    }
    #[test]
    fn reasoning_item_and_actual_retained_memory_have_independent_limits() {
        let (prefix, assistant, mut turn) = fixture();
        turn.assistant = json!([{ "type":"thinking","signature":"s".repeat(MAX_ITEM)}]);
        assert!(Pending::new([1; 32], &prefix, &assistant, turn).is_err());
        let (prefix, assistant, turn) = fixture();
        let raw = json_bytes(&turn.assistant);
        let pending = Pending::new([1; 32], &prefix, &assistant, turn).unwrap();
        assert!(pending.bytes > raw);
    }
}
