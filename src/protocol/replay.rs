//! 进程内 native 工具回放缓存：保存上游协议特有的 assistant 状态，供后续轮次
//! 把同一个工具调用还原为对应协议的原生结构。
//!
//! 上游调用方在 `proxy::native` 中完成三步：先由 `Pending::new` 构造尚未发布
//! 的候选状态，再由 `Cache::commit` 在响应成功边界写入本进程的缓存，最后在后续
//! 请求进入协议转换前调用 `Cache::resolve`。HTTP 层的 `CacheTicket` 只负责在
//! “尚未交给响应 body 层”时撤销本次发布；它不是本模块的类型，也不代表客户端
//! 已在网络上收到数据。
//!
//! 数据只保存在当前进程内存。源码没有把这里的 opaque state 写入 SQLite 或日志，
//! 因而进程退出、缓存过期、路由代次变化或容量回收后，旧调用不能再从持久存储
//! 恢复。缓存按工具调用 ID 建立索引；一个 `Entry` 和一个 `ReplayTurn` 代表整组
//! 工具调用，多个 ID 通过 `Arc<Entry>` 指向同一 `Entry`，该 `Entry` 持有一份
//! `Arc<ReplayTurn>` 句柄；`resolve` 返回历史时还会 clone 出额外句柄。同一个进程
//! 里的一个 `Bridge` 所管理的多条 route 共享其 `Cache`，条目可以共存，
//! 但 `resolve` 会用 `RouteKey` 校验当前有效代次。
//!
//! `Cache` 自身不是并发容器，也不创建线程；上层共享时把它放进互斥保护的范围，
//! 每个缓存方法本身都是同步操作，不会在锁内跨 `await`；`resolve` 的摘要计算也在
//! 锁内完成。它返回的 `ResolvedHistory` 含 `Arc<ReplayTurn>`，上层释放锁后才继续
//! 编码和 HTTP 发送。该历史会独立于缓存索引存活，因此即使条目已从 FIFO 和索引
//! 退休，只要后续请求仍持有这个 `Arc`，其容量仍计入预算。`retired` 中的 `Weak`
//! 只用于之后检查“强引用是否已经释放”，随后由显式 `collect_retired` 回收账目。
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
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

/// 缓存条目从 `Entry.created` 起算的固定寿命，单位秒。`resolve` 只读取条目，
/// 不会刷新 `created` 或重新插入 FIFO，因此命中不会把 TTL 向后滑动。
const TTL: Duration = Duration::from_secs(30 * 60);
/// 缓存容量预算中工具调用 ID 的上限，不是“对话轮数”或 `Entry` 数量。一个整组含
/// 多个 call ID 时按 `calls.len()` 占用多个名额；退休但仍被活跃请求钉住的整轮
/// 也继续占用这些名额。
const MAX_ITEMS: usize = 512;
/// 保守的驻留分配记账上限，单位字节。它不是进程 RSS 上限，也不是对分配器的
/// 硬保证；`allocation` 只为 `Value` 树估算容量和容器开销，其他分配不在此账内。
const MAX_BYTES: usize = 32 * 1024 * 1024;
/// 单个 `turn.assistant` 完整 JSON 序列化后的上限，单位字节。它只约束保存下来的
/// native assistant content 结构，不是完整上游响应包装、usage 或外层状态；也不是
/// 某个 item 的上限。
const MAX_RESPONSE: usize = 8 * 1024 * 1024;
/// `turn.assistant` 中每个顶层 item 的 JSON 序列化上限，单位字节。它与整组
/// `MAX_RESPONSE` 同时生效。
const MAX_ITEM: usize = 1024 * 1024;

/// 路由管理层的 effective generation 标识，由上层生成和维护；replay 模块只把
/// 这 32 字节作相等比较。它不是本模块对配置或凭据计算出的 SHA，也不表示可跨
/// 进程持久化或稳定的 key。代次不同即拒绝旧缓存。
pub type RouteKey = [u8; 32];

/// 为一次 native 工具调用生成公开给后一轮客户端的 opaque ID。
///
/// 输出固定为 `wbpt_` 前缀加 32 个小写十六进制字符，共 37 字节，随机部分
/// 为 16 字节（128 bit）。源码使用随机生成器，没有在生成函数内扫描全进程
/// 检测碰撞；`Cache::commit` 对已索引 ID 的重复会返回 `duplicate_tool_id`。
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

/// 已校验、尚未发布到 `Cache` 的一轮 native assistant 状态。
///
/// `proxy::native` 在确定上游输出可以形成 replay 状态后创建它。它不是缓存条目：
/// 在调用 `Cache::commit` 前失败或取消时，直接 drop `Pending` 即可，不会产生待清理
/// 的全局索引。一旦 `publish` 已把 `Pending` 交给 `commit`，HTTP 层就不再通过 drop
/// 这个候选对象撤销，而由尚未 delivered 的 `CacheTicket` 调用整轮 `discard`。
pub struct Pending {
    /// 生成该状态时的有效路由代次。
    route: RouteKey,
    /// 请求前缀与公开 assistant 工具消息的摘要绑定。
    binding: [u8; 32],
    /// native 工具轮次的共享本体；发布后由唯一 `Entry` 拥有这个 `Pending`，
    /// 多个 ID 共享 `Arc<Entry>`，后续命中再 clone `Arc<ReplayTurn>`。
    turn: Arc<ReplayTurn>,
    /// 本待发布状态预计占用的内部预算，单位字节。
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

/// `serde_json::to_writer` 的计数 sink。
///
/// `Write` 只累计实际写出的序列化字节，不生成或复制 JSON 字符串载荷。
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

/// 计算一个 JSON `Value` 序列化后的字节数，不创建字符串副本。
///
/// 序列化失败时返回 `usize::MAX`，让后续大小检查把它当作必然超限，而不是误报为
/// 一个可接受的较小值。
fn json_bytes(value: &Value) -> usize {
    let mut count = Counter::default();
    if serde_json::to_writer(&mut count, value).is_err() {
        return usize::MAX;
    }
    count.0
}

/// 估算一个 `Value` 树保守计入缓存预算的驻留分配量。
///
/// 公式按字符串和容器的 `capacity`、数组元素、对象节点等结构开销估算，不是 JSON
/// 序列化长度，也不是进程 RSS 的测量值；小对象较多的树通常比原始 JSON 字节更长。
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
    /// 构造尚未发布的 native 回放状态，并完成结构、大小与摘要绑定校验。
    ///
    /// 调用来源是 `proxy::native::pending`。`route` 表示当前有效路由代次；
    /// `prefix` 是公开 assistant 工具消息之前的消息历史；`assistant` 是准备
    /// 返回给客户端、含 `wbpt_` 工具调用的消息；`turn` 是对应协议的原生 assistant
    /// 内容，所有权会转移进返回的 `Pending`。
    ///
    /// 本函数只计算并构造对象，不写 `Cache`、网络或存储。它检查 assistant 角色、
    /// 非空且不超过 `TOOL_LIMIT` 的调用数、公开 calls 与 native calls 的对应关系、
    /// ID 形状，以及 `MAX_RESPONSE`、`MAX_ITEM` 和 `MAX_BYTES`。任一结构条件不成立
    /// 返回 502 `invalid_replay_state`；任一大小限制不成立返回 502
    /// `conversation_state_too_large`。成功表示“可以尝试发布”，不表示客户端已经
    /// 收到工具调用。
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

/// `Cache` 中一轮已发布 native 状态的根记录。
///
/// 同一轮的所有工具调用 ID 都索引到同一个 `Arc<Entry>`；因此按任一 ID 撤销时会
/// 找到并退休整轮。`created` 在提交时固定，后续命中不修改它；`resolve` 可取走
/// `ReplayTurn` 的额外 `Arc` 句柄，因此 `Entry` 内保存的不必是唯一句柄。
struct Entry {
    /// 已发布内容的共享载荷、路由与摘要绑定。
    pending: Pending,
    /// 该轮进入缓存的时间，也是固定 TTL 的起算点。
    created: Instant,
}

/// 当前进程 `Bridge` 共享的 native 回放缓存。
///
/// 索引和 FIFO 都只持有整轮记录。`bytes` 与 `items` 是两个独立配额：前者为保守
/// 分配记账，后者为工具调用 ID 数量。两种配额任一超限都会触发整轮 FIFO 回收。
/// 不同有效路由的条目可以同时存在；命中时由 `Pending.route` 的 `RouteKey` 校验，
/// 而不是为每个 route 创建独立 `Cache`。
///
/// 本结构没有内部 `Mutex`，也没有后台清理线程；所有公开操作要求调用方提供独占
/// `&mut self`。上层在共享状态互斥锁中调用它们，因此实际锁边界由上层决定。
#[derive(Default)]
pub struct Cache {
    /// 工具调用 ID 到整轮记录的查找表；一个 call 对应一个 `Arc` 句柄。
    by_id: BTreeMap<String, Arc<Entry>>,
    /// 按提交顺序记录整轮，用于 FIFO 回收；这里不是访问顺序，所以不是 LRU。
    oldest: VecDeque<Arc<Entry>>,
    /// Removed lookup entries can still be used by an in-flight request.
    /// Their allocations keep consuming the budget until the final Arc drops.
    /// Weak 只观察 `ReplayTurn` 的强引用数，不延长载荷寿命。
    retired: Vec<(Weak<ReplayTurn>, usize, usize)>,
    /// 当前仍计入预算的分配估算，包含退休但仍被活跃请求持有的载荷。
    bytes: usize,
    /// 当前仍计入预算的工具调用 ID 数，包含已索引记录和退休但尚未释放的整轮。
    items: usize,
}
impl Cache {
    /// 清除已经被所有活跃请求释放的退休记录，并归还其预算。
    ///
    /// `Weak` 不拥有 `ReplayTurn`；只有 `weak.strong_count()` 已降到 0 时才代表
    /// 最后一个活跃 `ResolvedHistory` 也已释放。`expire`、`commit`、`discard`
    /// 会触发本检查，因此释放之后还要有一次缓存操作才更新账目。
    fn collect_retired(&mut self) {
        self.retired.retain(|(turn, bytes, items)| {
            if turn.strong_count() > 0 {
                true
            } else {
                self.bytes = self.bytes.saturating_sub(*bytes);
                self.items = self.items.saturating_sub(*items);
                false
            }
        });
    }

    /// 退休当前 FIFO 最前端的一整轮。
    ///
    /// 队列为空时什么都不做。该函数不按访问热度挑选条目，也不复用命中顺序。
    fn evict_oldest(&mut self) {
        if let Some(entry) = self.oldest.pop_front() {
            self.retire_entry(entry);
        }
    }

    /// 从查找索引移除整轮，并决定立即释放预算还是登记为退休占用。
    ///
    /// 所有 call ID 都指向同一 `Entry`。若另有请求正持有该轮的 `Arc<ReplayTurn>`，
    /// 仅保存 `Weak` 并继续计费；载荷释放后，未来 `collect_retired` 才能扣账。
    fn retire_entry(&mut self, entry: Arc<Entry>) {
        for id in entry.pending.turn.calls.keys() {
            self.by_id.remove(id);
        }
        if Arc::strong_count(&entry.pending.turn) > 1 {
            self.retired.push((
                Arc::downgrade(&entry.pending.turn),
                entry.pending.bytes,
                entry.pending.turn.calls.len(),
            ));
        } else {
            self.bytes = self.bytes.saturating_sub(entry.pending.bytes);
            self.items = self.items.saturating_sub(entry.pending.turn.calls.len());
        }
    }

    /// 按任一工具调用 ID 撤销其整轮状态。
    ///
    /// HTTP `CacheTicket::Drop` 用它处理“已 publish、但尚未交给响应 body”的失败
    /// 边界。ID 不要求是首调用；命中后在 FIFO 中移除包含它的一整轮。未知或已退休
    /// ID 不报错，随后顺便清理可回收的退休账目。
    pub fn discard(&mut self, id: &str) {
        if let Some(position) = self
            .oldest
            .iter()
            .position(|e| e.pending.turn.calls.contains_key(id))
            && let Some(entry) = self.oldest.remove(position)
        {
            self.retire_entry(entry);
        }
        self.collect_retired();
    }

    /// 移除 `created` 已满固定 TTL 的前端整轮。
    ///
    /// 从 `created` 计算是确定的，`resolve` 命中不能延长寿命。正常提交使 FIFO
    /// 时间单调时，循环清掉队首过期的连续条目即可；与检索顺序无关。
    fn expire(&mut self, now: Instant) {
        self.collect_retired();
        while self
            .oldest
            .front()
            .is_some_and(|e| now.saturating_duration_since(e.created) >= TTL)
        {
            self.evict_oldest();
        }
    }

    /// 发布一轮状态，并在总量超限时按整轮 FIFO 腾出预算。
    ///
    /// 调用来源是 `proxy::native::publish`，只有上游响应成功并已生成缓存候选时
    /// 才执行，`now` 由该调用方提供。输入 `Pending` 的所有权在这里转移成缓存记录。
    ///
    /// 步骤依次是：固定 TTL 清理、重复 ID 检查、工具调用 ID 数/分配记账容量检查、
    /// 加入两个计数、建立 `Entry`、建立所有 ID 索引、压入 FIFO。重复 ID 返回
    /// 502 `duplicate_tool_id`；若仍超预算且队列已无可回收条目，返回 502
    /// `conversation_state_capacity_exhausted`，常见原因是退休载荷仍被活跃请求
    /// 持有。容量腾挪会真实移除旧条目，本函数不是跨步骤事务，失败路径不回滚此前
    /// 已完成的退休操作。
    ///
    /// 成功只表示状态已在进程内发布，不表示响应已交付客户端。
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
            || self.items + pending.turn.calls.len() > MAX_ITEMS
        {
            if self.oldest.is_empty() {
                return Err(Error::upstream("conversation_state_capacity_exhausted"));
            }
            self.evict_oldest();
        }
        self.bytes += pending.bytes;
        self.items += pending.turn.calls.len();
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

    /// 从当前消息历史解析所有可用的 native 工具轮次。
    ///
    /// 调用发生在后续 native 请求转换前。输入 `route` 必须与本次有效路由代次一致；
    /// `messages` 是客户端送回的统一公开历史。输出 `ResolvedHistory` 以消息下标为
    /// 键，保存对应 `Arc<ReplayTurn>`，转换器据此用原生 item 替换含 `wbpt_` 的
    /// assistant 工具调用消息。紧随其后的公开 tool-result 消息仍由公开历史携带，
    /// 本模块不会用 `ReplayTurn` 替换 tool-result 消息。
    ///
    /// 只有 assistant 消息至少含一个 `wbpt_` call 才触发这条恢复链。完全只含普通
    /// 外来 ID 的 assistant 消息不会报缺少缓存，而是按普通消息继续处理。一旦某条
    /// 含 `wbpt_` 的消息进入恢复：
    ///
    /// 1. 首 call ID 必须在当前索引中。
    /// 2. 所有 call ID 必须指向同一 `Arc<Entry>`。
    /// 3. `RouteKey`、整轮 call 数和截止该 assistant 的摘要都必须匹配。
    ///
    /// 缺失、已过期、已退休或已撤销，以及 route generation 不符，返回 409
    /// `conversation_state_expired`；跨 Entry、call 数，或目标 assistant 之前的
    /// 摘要/内容/工具结果分组不符，返回 400 `conversation_state_mismatch`。函数会
    /// 先执行固定 TTL 清理，但成功命中本身不改变 `created`。数据库、日志和网络
    /// 均无副作用。
    pub fn resolve(
        &mut self,
        route: &RouteKey,
        messages: &[Message],
        now: Instant,
    ) -> Result<ResolvedHistory, Error> {
        self.expire(now);
        let mut out = ResolvedHistory::new();
        if !messages
            .iter()
            .any(|m| m.role == Role::Assistant && m.calls.iter().any(|c| c.id.starts_with("wbpt_")))
        {
            return Ok(out);
        }
        let mut prefix = initial_hash();
        let mut i = 0;
        while i < messages.len() {
            let m = &messages[i];
            if m.role == Role::Tool {
                i = hash_result_group(&mut prefix, messages, i);
                continue;
            }
            if m.role != Role::Assistant || !m.calls.iter().any(|c| c.id.starts_with("wbpt_")) {
                hash_message(&mut prefix, m);
                i += 1;
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
                || finish_binding(prefix.clone(), i, m) != entry.pending.binding
            {
                return Err(invalid());
            }
            out.insert(i, entry.pending.turn.clone());
            hash_message(&mut prefix, m);
            i += 1;
        }
        Ok(out)
    }
}

/// 把带长度域分隔的字符串写入摘要，避免相邻字段边界产生歧义。
fn field(hash: &mut Sha256, s: &str) {
    hash.update((s.len() as u64).to_le_bytes());
    hash.update(s.as_bytes());
}

/// 把一条公开消息写入摘要规范流。
///
/// 消息角色也参与摘要。非 assistant 消息会保留 parts 数量和每个图片/文本部分的
/// 类型及内容；assistant 只纳入 `m.text()` 投影后的可见文本，不纳入 parts 数量。
/// 因而任意 text 分片只要投影结果相同就得到相同摘要；工具专用 assistant 的
/// `content: null` 与空字符串只是这个规则最常见的例子。每个工具调用依次写入
/// ID、名称和解析后的 JSON `arguments`；call 循环结束后，再为整条 message 写入
/// 一次 `tool_call_id`。参数从 `Value` 重新序列化，可消除对象键顺序及无意义空白
/// 差异。
fn hash_message(hash: &mut Sha256, m: &Message) {
    hash.update([match m.role {
        Role::System => 0,
        Role::Developer => 1,
        Role::User => 2,
        Role::Assistant => 3,
        Role::Tool => 4,
    }]);
    if m.role == Role::Assistant {
        // Standard clients may round-trip a tool-only assistant content as
        // either null or an empty string. Both bind to the same visible text.
        field(hash, &m.text());
    } else {
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

/// 计算“完整请求前缀 + 本轮公开 assistant”的 32 字节绑定摘要。
///
/// 构造 `Pending` 和后一轮 `resolve` 使用同一套规范化规则；任一会影响工具轮次
/// 身份的历史内容、调用或结果分组发生变化，摘要都不匹配。
fn binding(prefix: &[Message], assistant: &Message) -> [u8; 32] {
    let mut hash = initial_hash();
    let mut i = 0;
    while i < prefix.len() {
        if prefix[i].role == Role::Tool {
            i = hash_result_group(&mut hash, prefix, i);
        } else {
            hash_message(&mut hash, &prefix[i]);
            i += 1;
        }
    }
    finish_binding(hash, prefix.len(), assistant)
}

/// 建立带域版本标识的 SHA-256 初始状态。
///
/// `v2` 明确区分当前摘要语义；只有消息/调用摘要的规范化规则变化时才需要评估
/// 版本升级。无关协议字段或编码规则变化不必因此修改该常量。
fn initial_hash() -> Sha256 {
    let mut hash = Sha256::new();
    hash.update(b"workbuddy-native-replay-v2");
    hash
}

/// 把前缀长度和本轮 assistant 写入摘要并生成最终 32 字节值。
///
/// 前缀长度是显式域，避免不同消息数量但拼接内容碰巧相同的歧义。
fn finish_binding(mut hash: Sha256, prefix_len: usize, assistant: &Message) -> [u8; 32] {
    hash.update((prefix_len as u64).to_le_bytes());
    hash_message(&mut hash, assistant);
    hash.finalize().into()
}

/// 规范写入一段连续的 tool-result 消息，并返回该组之后的下标。
///
/// 同一 assistant 的多个结果通常由并行工具产生，客户端可能按不同完成顺序排列。
/// 这里先收集连续 `Role::Tool` 消息，再按 `tool_call_id` 字符串排序后逐条写入
/// 摘要；实践语义是按 ID 配对成组，而非把客户端数组顺序当作身份。
fn hash_result_group(hash: &mut Sha256, messages: &[Message], start: usize) -> usize {
    let end = start
        + messages[start..]
            .iter()
            .take_while(|m| m.role == Role::Tool)
            .count();
    let mut group = messages[start..end].iter().collect::<Vec<_>>();
    group.sort_by(|a, b| a.tool_call_id.cmp(&b.tool_call_id));
    for m in group {
        hash_message(hash, m);
    }
    end
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

    #[test]
    fn evicted_state_held_by_an_active_request_still_consumes_the_budget() {
        let (mut prefix, assistant, turn) = fixture();
        let now = Instant::now();
        let route = [1; 32];
        let mut cache = Cache::default();
        let mut pending = Pending::new(route, &prefix, &assistant, turn).unwrap();
        // Exercise the accounting boundary without allocating 32MiB in a test.
        pending.bytes = MAX_BYTES;
        prefix.push(assistant);
        cache.commit(pending, now).unwrap();
        let held = cache.resolve(&route, &prefix, now).unwrap();
        let (prefix, assistant, turn) = fixture();
        let pending = Pending::new(route, &prefix, &assistant, turn).unwrap();
        assert_eq!(
            cache.commit(pending, now).unwrap_err().code,
            "conversation_state_capacity_exhausted"
        );
        assert_eq!(cache.bytes, MAX_BYTES);
        assert!(cache.by_id.is_empty());
        drop(held);
        cache.expire(now);
        assert_eq!(cache.bytes, 0);
        assert_eq!(cache.items, 0);
        let (prefix, assistant, turn) = fixture();
        cache
            .commit(Pending::new(route, &prefix, &assistant, turn).unwrap(), now)
            .unwrap();
    }
}
