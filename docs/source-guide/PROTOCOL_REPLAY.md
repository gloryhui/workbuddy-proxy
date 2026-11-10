# Native 工具回放：opaque state、摘要与交付边界

本文沿 `src/protocol/replay.rs` 解释 WorkBuddy Proxy 为什么要在进程内保存一段 native
assistant 状态，以及后续 Chat 请求如何把同一次工具调用还给 OpenAI Responses、
Anthropic Messages 或 Gemini。请求侧的公开 DTO 和校验先读
[Native 请求转换](PROTOCOL_REQUEST.md)；上游响应如何解码、公开 assistant 消息
如何形成先读 [原生协议响应适配](PROTOCOL_RESPONSE.md)。服务启动、直通和 native
分支的整体位置见 [服务核心源码流程](SERVICE_CORE.md)；四篇专题的阅读路线见 [协议转换总览](PROTOCOL_CONVERSION.md)。

这里讨论的是工具调用跨轮次的数据连续性。`Cache` 不保存用户会话正文数据库，不把
opaque state 写入 SQLite 或日志，也没有后台清理线程。理解这条链时，最重要的是
把下面四件事分开：

1. `Pending` 是尚未发布的候选状态。
2. `Cache::commit` 表示状态已进入当前进程缓存。
3. `CacheTicket` 表示尚未交付的缓存发布仍可撤销；流式响应此前发出的文本不会因此撤回。
4. 客户端在网络层实际收到多少字节，不属于 replay 模块能证明的范围。

## 1. 真实调用顺序

native 响应成功形成完整 `Output` 后，`src/proxy_native.rs` 的 `pending()` 会：

1. 调用 `out.validate_request(p)` 检查响应是否满足本次请求约束。
2. 用 `out.replay.take()` 取走上游原生 assistant 内容。
3. 调用 `replay::Pending::new(r.generation, &p.messages, &out.assistant, turn)`。

第三步只在 `out.replay` 为 `Some(turn)` 时执行。没有工具调用的响应无需发布回放状态，最终返回 `None`。

随后 `publish()` 调用 `Bridge.native_replay.lock()`，在互斥保护范围内执行
`Cache::commit`，并返回 `CacheTicket { delivered: false }`。后续请求进入 native
转换前，会再次锁住同一个缓存并调用 `Cache::resolve`。

```text
上游响应完整解析
    -> Output.replay: Option<ReplayTurn>
    -> Pending::new(...)
    -> Cache::commit(...)
    -> CacheTicket { delivered: false }
    -> 响应 body 层取到完整 payload / data: [DONE]
    -> CacheTicket 不再撤销缓存发布

后续请求
    -> 公开 messages 含 wbpt_ call
    -> Cache::resolve(...)
    -> ResolvedHistory[index] = Arc<ReplayTurn>
    -> 协议编码器用原生 item 还原历史
```

`Cache` 本身不创建线程，也不包含 `Mutex`。它要求 `&mut self`，上层把整个缓存放进
`Bridge` 的进程内互斥状态。同一个 `Bridge` 管理的多个有效 route 共用其 `Cache`，条目
可以并存；`resolve` 再用 `RouteKey` 判断当前 route 是否匹配。不同进程各有自己的
`Bridge` 和缓存，不存在跨进程内存共享。

缓存方法是同步的，互斥锁不跨 `await`。`resolve` 会在锁内完成历史扫描和摘要计算，
返回含 `Arc<ReplayTurn>` 的 `ResolvedHistory` 后，上层释放锁，再继续协议编码和 HTTP
输出。`RouteKey` 由上层路由管理生成和维护；replay 只对两个 32 字节值做相等比较，
它没有在模块内根据配置或凭据计算路由哈希。

## 2. `Pending`、`Entry` 和 `ReplayTurn` 的所有权

`ReplayTurn` 定义在 `src/protocol/encode.rs`，包含：

- `assistant: serde_json::Value`：要还给上游协议的 native assistant 内容。
- `calls: BTreeMap<String, NativeCall>`：公开 `wbpt_` call ID 到原生 ID/name 的映射。

`Pending::new` 接收 `turn: ReplayTurn` 的值。参数传入函数后所有权转移，函数把它
包成 `Arc<ReplayTurn>`，连同 `RouteKey`、摘要 `binding` 和预算 `bytes` 放进
`Pending`。这时还没有进入全局索引；若响应在 `commit` 前失败，局部 `Pending` 被丢弃
即可。一旦 `publish` 已执行 `commit`，HTTP 层不能再靠丢弃 `Pending` 撤销状态；未交付
响应由 `CacheTicket::Drop` 调用 `discard` 处理。

`Cache::commit` 接收 `Pending` 的值，创建 `Entry`。一个 `Entry` 代表整轮工具调用，
其中：

- `pending` 持有一份 `Arc<ReplayTurn>` 句柄；之后 `resolve` 还能 clone 出额外句柄。
- `created` 固定记录提交时刻。
- `by_id` 中每个 call ID 都保存一个 `Arc<Entry>` 句柄。
- `oldest` 中再保存一个 `Arc<Entry>`，用于整轮 FIFO。

一个工具轮次有两个 call 时，`by_id` 有两条键，但它们指向同一个 `Arc<Entry>`。该
唯一 `Entry` 持有一份保存 `ReplayTurn` 的 `Arc` 句柄。因此从任一 ID 出发都能找到
整轮，不存在“只删掉一个工具”的正常操作。

Rust 的 `Arc<T>` 是带原子引用计数的共享所有权句柄。克隆 `Arc` 通常只增加计数，
不会自动深拷贝 `T`；“是否会真正复制内部对象”取决于 `T` 和代码如何构造它。
`Arc<Entry>` 被多个索引共同持有时，最后一个句柄释放才 drop `Entry`。

`Pending` 成功提交后，它的所有权已经进入 `Entry`。`commit` 返回 `Ok(())` 后，
调用方不再持有原始 `ReplayTurn`；缓存是否还能被解析，由后续 `resolve` 和容量状态
决定。

## 3. `Arc`、`Weak` 与退休条目

缓存命中时，`ResolvedHistory` 保存 `entry.pending.turn.clone()`，也就是取得另一个
`Arc<ReplayTurn>` 句柄。协议转换可能在释放缓存锁后继续使用这段内容，所以缓存条目
不能假设自己一退休，内容就立刻消失。

`retire_entry` 先删除该轮所有 `by_id` 键，然后比较
`Arc::strong_count(&entry.pending.turn)`：

- 如果只有 `Entry` 自己持有，强引用计数为 1，立即扣回 `bytes` 和 `items`。
- 如果已有请求持有 `ResolvedHistory`，计数大于 1，则向 `retired` 放入
  `Weak<ReplayTurn>`、该轮预算和 call ID 数。

`Weak<T>` 不增加强引用计数，也不延长 `ReplayTurn` 的寿命。它只让缓存之后检查
载荷是否还存在。只有最后一个活跃 `Arc<ReplayTurn>` 释放后，未来的
`collect_retired()` 才能看到 `strong_count() == 0` 并扣回预算。

这与“取消后立即释放全部内存”不同。`expire`、`commit` 和 `discard` 会尝试清理
退休账目，但若最后一次缓存操作发生在活跃请求释放之前，账目要等后续操作才更新。
因此可能出现：

```text
by_id: 空
oldest: 空
bytes/items: 仍大于 0
retired: 仍有 Weak 对应活跃 ReplayTurn
```

此时新提交无法从 FIFO 再淘汰普通条目，可能返回
`conversation_state_capacity_exhausted`。这是对真实仍被引用的载荷继续计费，不是
永久泄漏，也不保证下一次恰好能回收；回收时机取决于强引用释放和后续缓存操作。

## 4. TTL、FIFO 和三个大小限制

`TTL` 是 30 分钟。`Entry.created` 在 `commit` 时写入，之后不会被 `resolve`
修改。命中只读取状态，不会刷新创建时间，也不会把条目移到队尾。因此 TTL 从第一次
发布起固定计算，不因使用而滑动。

`expire(now)` 在 `resolve` 和 `commit` 前调用，检查 `created` 到 `now` 是否达到
TTL。正常提交顺序下，`VecDeque` 队首最老；超过 TTL 的整轮从队首退休。它不是按最后
访问时间排序，所以整个缓存是 FIFO，而不是 LRU。

容量有两套独立预算：

| 配额 | 当前值 | 精确含义 |
| --- | ---: | --- |
| `items` | 512 | 仍在计费的工具 call ID 数，不是 turn 数 |
| `bytes` | 32 MiB | 代码估算的保守驻留分配 charge，不是进程总 RSS |

一个整轮若有 3 个 call，会向 `items` 加 3。`commit` 先检查当前预算加上新轮次是否
超限，再不断退休 FIFO 队首整轮，直到两个配额都能容纳，或队列已经为空。
退休但被活跃请求钉住的整轮仍同时占用 `items` 和 `bytes`。

32 MiB 不能理解成“进程最多使用 32 MiB 内存”。`allocation()` 针对 `Value` 树的
字符串容量、数组容量、map 容器和节点开销做内部估算，再另加 `ReplayTurn` 与 call
映射的估算。分配器元数据、其他请求对象、其他缓存和运行库内存都不在这笔账中。源码
也没有承诺该公式与实际 RSS 精确相等或具有固定上界。

另外还有两个作用于“已保存 native assistant 内容”的 JSON 大小边界：

- 整个 `turn.assistant` 序列化后不能超过 8 MiB。
- 每个顶层 native assistant item 序列化后不能超过 1 MiB。

8 MiB 不统计完整上游响应包装、usage 或外层状态，只统计 `turn.assistant` 这个保存结构。
一个 item 不能因为整包尚小于 8 MiB 就无限增长；两个限制独立生效。`Pending::new`
还检查 `MAX_BYTES`，以上任一 size 边界失败都返回
`conversation_state_too_large`。

## 5. `commit` 与整轮撤销

`Cache::commit` 的顺序是：

1. 按 `now` 清理过期状态和可回收的退休账目。
2. 检查新轮的任一 call ID 是否已出现在 `by_id` 中。
3. 按 `bytes` 和 `items` 退休 FIFO 队首，直到容量足够。
4. 增加 `bytes/items`。
5. 创建 `Entry`，用全部 call ID 建立索引，再把整轮压入队尾。

重复 ID 返回 502 `duplicate_tool_id`。容量腾挪会实际退休旧条目；如果之后仍无法
容纳且队首已空，返回 502 `conversation_state_capacity_exhausted`。这条路径没有
事务式回滚：已退休的旧状态不会因为最后失败而重新插入，也不存在数据库事务需要提交
或回滚。

`discard(id)` 从 `oldest` 中找到包含该 ID 的整轮并退休全部 call。ID 可以是任意一个
call，不要求是第一条。未知 ID 不报错，随后仍会尝试 `collect_retired()`。HTTP
`CacheTicket::Drop` 在“已 publish、尚未 delivered”时调用的正是这个整轮撤销入口。

## 6. `resolve` 如何证明“这是同一轮”

`resolve` 先执行固定 TTL 清理。随后快速检查历史里是否存在 assistant 消息，且至少
一个 call ID 以 `wbpt_` 开头。

完全只有普通外来 call ID 的 assistant 历史不会触发 replay，也不会因为没有缓存
而报错。只有在含 `wbpt_` 的消息进入恢复链后，当前组的全部 call 都按缓存身份处理；
手工构造“一个 `wbpt_` 加一个外部 ID”的混合组不是可用的回退路径。

对每个触发恢复的 assistant 消息，源码依次校验：

1. 首个 call ID 可在 `by_id` 找到。
2. 当前 route generation 与 `Pending.route` 相同。
3. 组内全部 call ID 都可在 `by_id` 找到。
4. 每个 call 映射到的 `Arc<Entry>` 与首个 call 相同；比较使用 `Arc::ptr_eq`。
5. 当前 assistant 的 call 数等于整轮 native calls 数。
6. “从请求开始到该 assistant 之前的前缀，再加该 assistant”的规范化摘要等于
   `Pending.binding`。目标 assistant 之后的 tool-result 不参与它的绑定。

“状态还在”并不够。若第一条 call 来自旧轮、第二条来自另一轮，即使两个 ID 都存在，
也会因不指向同一个 `Entry` 而失败。若 route generation 变化，相同 ID 也不能跨有效
配置复活。

错误分层如下：

| 情况 | 状态码 | 错误码 |
| --- | ---: | --- |
| ID 不存在、已过期、已退休或被撤销 | 409 | `conversation_state_expired` |
| route generation 不符 | 409 | `conversation_state_expired` |
| call 跨不同 `Entry` | 400 | `conversation_state_mismatch` |
| call 数不一致 | 400 | `conversation_state_mismatch` |
| 目标 assistant 之前的历史内容、工具参数、结果分组或摘要不一致 | 400 | `conversation_state_mismatch` |

这些操作没有数据库或日志副作用。`resolve` 在返回错误前可能已清理过期或可回收状态；
错误信封不会向客户端泄漏 opaque state。

## 7. 摘要绑定：到底规范化了什么

`binding` 的 SHA-256 在本模块计算，使用的初始域字符串是
`workbuddy-native-replay-v2`。它与上层提供的 `RouteKey` 是两种不同的绑定：

- `RouteKey` 表示 effective route generation，本模块只比较字节。
- `binding` 表示公开消息历史与公开 assistant 工具轮的内部摘要。

每个字符串字段写入前都带 UTF-8 字节长度，避免“前字段末尾与后字段开头相同”造成拼接
歧义。消息摘要包含角色，assistant 和其他角色使用不同规则：

- 非 assistant 消息包含 parts 数量和每个 Text/Image 部分的内容。
- assistant 消息使用 `m.text()` 的可见文本投影，不包含 parts 数量。
  `m.text()` 把 text 合成为无额外分隔符的投影，所以不同 text 分片只要得到同一投影，
  摘要就相同；工具专用 assistant 的 `content: null` 与空字符串是典型例子，但规则
  不限于 tool-only 内容。

每条消息随后写入 call 数量；对每个 call，依次写入：

1. `id`
2. `name`
3. 解析后的 JSON `arguments`

call 循环结束后，源码再为整条 message 写入一次消息级 `tool_call_id`。

`ToolCall` 同时保存 `arguments: Value` 和 `arguments_text: String`。摘要使用解析后的
`Value` 重新序列化，不直接使用原始文本。对象键顺序或无意义空白不同但 JSON 语义
相同的参数可得到相同摘要；原始 `arguments_text` 仍由请求/响应协议适配逻辑序列化或
保留，不能理解成只用于日志。

连续 tool-result 消息组成一个 group。源码先截取连续 `Role::Tool`，再按
`tool_call_id` 排序后写摘要。并行调用完成顺序不同不应改变轮次身份，但非连续的 tool
消息属于不同 group，整体历史结构仍会影响摘要。

最后还会写入 assistant 前的消息数量，再写入本轮 assistant 摘要。因此“内容相同但
前面少一条消息”也不会共享同一个 binding。

## 8. `wbpt_` ID 的含义和限制

`tool_id()` 生成：

```text
wbpt_ + 32 个小写十六进制字符
```

前缀 5 字节加 32 字节文本，总长度 37。随机部分是 16 字节，即 128 bit。生成函数
没有在函数内部扫描整个缓存验证碰撞；`commit` 只检查当前 `by_id` 是否已有相同 ID。
因此不能把“128 bit 随机”扩展成概率计算未证明的绝对唯一保证。

`Pending::new` 校验 ID 以 `wbpt_` 开头、长度为 37，后 32 个位置是 ASCII
hexdigit；生成器本身输出小写。这个 ID 是客户端下轮回传的不透明关联键，不是用户 ID、
会话数据库主键、认证凭据或跨进程稳定标识。

## 9. 一次合成的多轮工具示例

下面是语义示例，ID 和值均为合成数据。假设第一轮客户端请求上海和北京天气，公开
assistant 消息包含两个 call：

```json
{
  "role": "assistant",
  "content": null,
  "tool_calls": [
    {
      "id": "wbpt_0123456789abcdef0123456789abcdef",
      "type": "function",
      "function": {
        "name": "get_weather",
        "arguments": "{\"city\":\"Shanghai\",\"unit\":\"c\"}"
      }
    },
    {
      "id": "wbpt_fedcba9876543210fedcba9876543210",
      "type": "function",
      "function": {
        "name": "get_weather",
        "arguments": "{\"city\":\"Beijing\",\"unit\":\"c\"}"
      }
    }
  ]
}
```

客户端执行工具后，可能按完成顺序回传两条连续 tool 消息，例如先北京、后上海，每条
都带对应 `tool_call_id`。下一轮 messages 再包含新的 user 消息并向上游请求。

第一轮发布时，缓存中的 `Entry`：

- 在 `by_id` 建立上述两个 ID，二者指向同一 `Entry`。
- 在 `oldest` 记录整轮 FIFO 位置。
- `turn.assistant` 保存上游能理解的原生 item。
- `turn.calls` 记录公开 ID 到上游 call ID/name 的映射。

第二次请求中的 `resolve` 扫描到公开 assistant 消息 A1，验证两个 ID 属于同一
`Entry`、route 相同、call 数相同，并重算 `U1 + A1` 的摘要。A1 后面的 tool-result
组尚未进入 A1 自己的 `Pending.binding`，所以不在这次校验中。校验成功后，
`ResolvedHistory` 在 A1 的消息下标处保存 `Arc<ReplayTurn>`；连续 tool-result 消息
仍由公开历史携带，不会被 `ReplayTurn` 替换。

如果第二轮响应 A2 又包含工具调用，它也会保存自己的回放状态：

```text
第一轮请求：U1
第一轮响应：A1（发布时绑定 U1 + A1）
客户端执行：T1, T2（A1 的结果）
第二轮请求：U1, A1, T1, T2, U2
第二轮响应：A2（含新工具调用，发布时绑定 A2 之前的完整 prefix + A2）
```

后续 `resolve` 校验 A1 时仍只看 `U1 + A1`；校验 A2 时，工具结果 T1/T2 才作为
A2 之前的历史进入 A2 的绑定。每个已缓存 assistant 各自绑定截止自己的 prefix。

协议编码器在该下标使用 native assistant item，而不是把公开 `wbpt_` 工具调用简单
透传。各协议具体字段映射由响应转换和编码器负责；本模块只保证“还回哪一段 opaque
state 是正确的”。

如果客户端只送还一个 `wbpt_` call，另一个被删掉，call 数检查返回 400。如果客户端
把两个 ID 拼到来自不同轮次的 assistant 消息中，`Arc::ptr_eq` 检查返回 400。如果整轮
已过 TTL 或已被容量回收，首个 ID 查不到时返回 409。

## 10. publish 与 delivery ticket 不是同一件事

`publish()` 的 `commit` 成功时，状态已经进入当前进程缓存，`CacheTicket.delivered`
仍是 `false`。这意味着“已发布但尚未交给响应 body”。

非流式路径先完整解析上游响应、校验并 commit，然后创建包含 payload 的 body stream。
body stream 第一次被轮询并把完整 payload 交给下一层时，ticket 被标记 delivered。
在这之前 body 被取消或丢弃，ticket 的 `Drop` 会锁定缓存并 `discard(id)`。

流式路径先暂存 ticket。转换器成功结束并生成控制帧后，ticket 等到
`data: [DONE]\n\n` 真正被 `next()` 取走并返回给 body 层时才标记 delivered。更早失败
会清空 ticket，触发未交付撤销。

三个边界应严格区分：

| 边界 | 能证明什么 | 不能证明什么 |
| --- | --- | --- |
| `Cache::commit` | 本进程缓存已有状态 | 响应 body 已开始输出 |
| ticket marked delivered | payload 或 DONE 已交给 HTTP body 层 | 客户端网络已收到 |
| TCP/客户端 ACK | 更下层传输事件 | replay 模块并未观察它 |

所以“publish 成功”不能用来记录“客户端已收到工具调用”；“ticket delivered”也不能
解释成客户端应用层确认。它们只服务于本地回放状态的保留/撤销。

## 11. Java / Spring Boot 对照

| Rust 结构 | 可用来帮助理解 | 关键差异 |
| --- | --- | --- |
| `Pending` | 尚未交给 Repository 的候选 DTO/状态对象 | 发布时 move 所有权，不需要 GC 等待回收 |
| `Cache` | 手写进程内缓存组件 | 无自动代理、无后台清线程，调用方显式加锁 |
| `Arc<T>` | 多个句柄共享一个堆对象 | 原子引用计数不等于自动深拷贝或 GC |
| `Weak<T>` | 非拥有观察者 | 不能当普通可解引用的对象，要显式检查寿命 |
| `&mut Cache` + 上层 `Mutex` | 互斥临界区 + 独占借用 | `&mut` 本身不负责线程同步 |
| `CacheTicket::Drop` | `try/finally` 式撤销钩子 | 由 drop 隐式触发，只撤销尚未交付的缓存发布 |
| binding SHA-256 | 内部状态指纹/版本绑定 | 不是 HTTP ETag，也不对客户端公开 |

把 `Mutex<Cache>` 粗略看成“同一时刻一个线程访问缓存”的临界区可以入门，但 Rust
借用检查、生命周期和 drop 语义仍是独立概念。同样，`Arc` 解决共享所有权，不解决
内部可变性；本模块依靠外层锁和 `&mut self` 串行修改。

## 12. 建议断点与现有测试

### 12.1 源码断点

建议按真实路径设置：

| 位置 | 观察内容 |
| --- | --- |
| `src/protocol/replay.rs` `Pending::new` | `assistant.calls`、`turn.calls`、三项 size 检查 |
| `src/protocol/replay.rs` `Cache::commit` | 运行前后 `bytes/items`、重复 ID、FIFO 退休 |
| `src/protocol/replay.rs` `retire_entry` | `Arc::strong_count` 是否大于 1 |
| `src/protocol/replay.rs` `collect_retired` | `Weak::strong_count` 何时归零 |
| `src/protocol/replay.rs` `Cache::resolve` | `Arc::ptr_eq`、route、call 数和 binding |
| `src/protocol/replay.rs` `hash_message` | role、assistant text 投影、canonical arguments |
| `src/protocol/replay.rs` `hash_result_group` | 连续 tool 消息及按 ID 排序 |
| `src/proxy_native.rs` `publish` | commit 成功但 delivered 仍为 false |
| `src/proxy_native.rs` `CacheTicket::drop` | 未交付撤销是否调用 `discard` |
| `src/proxy_native.rs` `NativeStream::next` | DONE 返回前设置 delivered |

注意 native 的 Rust 模块名是 `proxy::native`；对应源码文件是
`src/proxy_native.rs`。测试过滤器、日志断点和阅读路径应使用真实 Rust 符号
`proxy::native::*`，不要写成不存在的 `proxy_native::*`。

### 12.2 replay 单元测试

`src/protocol/replay.rs` 当前有四个直接测试：

- `protocol::replay::tests::pending_cancel_never_publishes_state_and_retries_can_reuse_committed_state`
- `protocol::replay::tests::expiry_and_capacity_eviction_are_bounded_and_discard_whole_groups`
- `protocol::replay::tests::reasoning_item_and_actual_retained_memory_have_independent_limits`
- `protocol::replay::tests::evicted_state_held_by_an_active_request_still_consumes_the_budget`

第一个测试覆盖“未 commit 不可 resolve”、发布后重复解析、route 不符 409 和内容篡改
400。第二个直接覆盖 512 call-ID 配额、整轮淘汰、TTL 到期回收和 `by_id/bytes` 状态；
它没有专门构造命中顺序来证明 LRU 差异。第三个验证单 item 与实际分配估算的独立边界。
第四个验证活跃 `Arc` 让退休状态继续占预算。

### 12.3 跨模块入口

还有两个很有价值的现有测试：

- `protocol::streaming::tests::standard_chat_history_replays_two_tool_rounds_without_private_fields`
- `proxy::native::tests::undelivered_replay_ticket_is_revoked_and_delivered_ticket_survives`

前者可观察公开历史中的两轮工具回放，确认普通 Chat 客户端不需要理解私有字段。后者
分别检查 ticket 未交付时撤销、已交付时保留，是串联 `publish`、`CacheTicket::Drop`
和 `Cache::resolve` 的最直接断点。

## 13. 阅读边界

replay 负责“哪种进程内 opaque state 可以还给哪种有效路由”，不负责 HTTP 会话数据库，
不负责把 SSE 字节发送到客户端，也不提供跨进程恢复。上游 Item、SSE decoder、各协议
字段转换和总调用链由相邻专题说明；沿本文先确认所有权、TTL、摘要和 ticket 边界，
再进入具体协议字段，通常最容易定位一次回放失败发生在哪一层。
