# GPUI 设置界面核心导读

本文面向熟悉 Java / Spring Boot、但不熟悉 Rust 的读者，按真实调用链解释
WorkBuddy Proxy 设置界面的启动、状态、供应商操作和统计查询。目标是能沿源码
回答四个问题：

1. 代码在哪个进程、哪个线程运行？
2. 当前状态由哪个对象拥有，谁能修改它？
3. 一次点击经过哪些队列和 HTTP 边界，失败发生在哪一侧？
4. 一个迟到的结果为什么可以安全丢弃？

## 1. 先分清三个桌面角色

桌面模式至少有三个角色：

| 角色 | 主要入口 | 责任 |
| --- | --- | --- |
| 托盘控制进程 | [src/frontend.rs](../../src/frontend.rs)、[src/gpui_host.rs](../../src/gpui_host.rs) | 管理 Core 生命周期、托盘行为，并按需启动设置进程 |
| 后台 Core 进程 | [src/main.rs](../../src/main.rs)、[src/proxy.rs](../../src/proxy.rs) | 代理请求、保存配置、执行统计查询，并接收本机管理 HTTP |
| GPUI 设置进程 | [gpui/src/main.rs](../../gpui/src/main.rs) | 显示设置窗口，持有界面 Entity，向 Core 发送管理命令 |

设置进程相对 Core 和托盘控制器分别都是独立操作系统进程。不要把这组关系
简化成“只有两个进程”，也不要仅凭设置入口推断另外两角色在每种平台上是否
复用或拆分成更多实例。三者通过 `instance.json`、`gpui.lock`、
`gpui-wake.json`、`gpui-stop.json` 等文件以及本机管理 HTTP 协作。

[gpui/src/main.rs](../../gpui/src/main.rs) 中的 `#[path = "../../src/..."]`
让 Cargo 在编译设置 crate 时复用源码文件。Core 与设置进程会分别编译并持有
自己的 `Config` 和 DTO；Entity 则属于设置进程的 GPUI 应用。编译期共享源码
不代表运行时共享堆对象，更不代表两边能直接同步修改同一个 Rust 值。

## 2. Rust 概念对应到本项目

### `Entity`、`Context` 和显式装配

`Entity<T>` 是 GPUI 应用上下文登记、可安全引用的实体句柄。`cx.new` 显式
创建 Entity，`Context<Self>` 把当前实体的更新和通知能力交给回调。可把
`Proxy::new`、`Backend::new`、`Statistics::new` 看成显式装配函数，但这里没有
Spring 的类型扫描、Bean 注入或事务代理。知道一个类型名，不会自动得到实例。

`Entity::clone()` 只按该句柄类型的契约克隆引用能力，不表示把 `Proxy` 的
草稿、分页和输入框整体深拷贝。需要反向观察父页面时，项目使用
`Entity::downgrade()` 得到 `WeakEntity`，再通过 `update` 访问；目标已销毁时
更新失败，从而避免父子 Entity 形成所有权环。

这些核心文件没有用 `Arc` 包装界面 Entity。其他地方若出现 `Arc`，它只表示
共享所有权；`Arc::clone` 通常增加引用计数，也不保证内部数据深拷贝。Rust
`Clone` 的深浅和成本完全取决于具体实现，不能照搬 Java“对象赋值都是引用”
的判断。

### 闭包、`move` 和借用

`move` 闭包取得其捕获值的所有权，适合把数据交给 GPUI callback、channel
worker 或异步任务。`this.update_in(cx, |this, window, cx| ...)` 中的 `this`
是回到特定 Entity 后的可变界面状态；后台 timer 先等待，再把执行切回 UI。

`&T` 是共享借用，`&mut T` 是排他可变借用；编译器检查冲突，`&mut` 本身不是
锁。实际读代码时：

- `input.read(cx).value()` 借用输入 Entity 的当前文本；
- `input.update(cx, |state, cx| ...)` 在输入 Entity 自己的 Context 中修改；
- `Context<Self>` 既表示当前 Entity，也提供 `notify/subscribe/spawn_in` 等 UI 能力。

### `Option`、`Result` 和枚举 `match`

`Option<T>` 的 `None` 不只等于空字符串。例如：

- `Reply.config=None` 表示这次动作没有可信任配置，不能拿旧值猜磁盘；
- `Reply.snapshot=None` 需要结合 tag 解释，某些 tag 本来就不附 snapshot；
- `Pending.source_id=None` 表示新建而非复制；
- `Dashboard=None` 表示尚无成功 dashboard 数据，不应显示为零。

`Result<T,E>` 把成功和失败放在类型里。`match` 必须穷尽枚举分支，例如
`Page`、`CoreState`、`Status`、`JobKind`、`ReplyRoute`。这类似 Java
`sealed interface + record` 的模式匹配，但业务分支由当前 Rust 源码实际枚举
决定。

### Channel 和事件通知

`sync_channel` 是有界队列。UI 使用非阻塞 `try_send/try_recv`，后台线程使用
阻塞 `recv/send`。channel 传递的是消息所有权，不共享 `Proxy` 本身。后台
完成后不能直接调用 `cx.notify()`；它把 Reply 放入队列，UI 的 `tick` 在正确
Context 中合并状态，然后调用 `notify()` 标记重绘。

`cx.notify()` 只发出“这个 Entity 需要重绘”的通知，不保证立即执行 render，
也不等于持久化或业务成功。

## 3. 推荐阅读顺序

先读供应商管理主线：

1. [main.rs](../../gpui/src/main.rs)：`main → application().run → open_window`。
2. [state.rs](../../gpui/src/state.rs)：`Draft`、`Drafts`、`Selection`。
3. [ui.rs](../../gpui/src/ui.rs)：`Proxy` 字段、`Proxy::new`、`capture`、
   `save_steps`、`send`、`test`、`tick`、`update_config`。
4. [backend.rs](../../gpui/src/backend.rs)：`Job`、`Reply`、`execute`、
   `call_detailed`、`Backend::new`。
5. [config_refresh.rs](../../gpui/src/config_refresh.rs)：外部配置如何安全合并。
6. [provider_create.rs](../../gpui/src/provider_create.rs)：本地新供应商如何
   绑定 Core 生成的真实 ID。
7. [provider_connection.rs](../../gpui/src/provider_connection.rs)：显式 Test
   收据何时变绿、何时失效。

再读统计页：

1. [statistics.rs](../../gpui/src/statistics.rs)：`Statistics` 字段、
   `Statistics::new`、`set_visible`、`mark_dirty`、`request_*`、
   `refresh_report`、`tick`、`handle_reply`、`apply_reply`。
2. [stats_controller.rs](../../gpui/src/stats_controller.rs)：`ReplyGate`、
   `FilterCoordinator`、`route_reply`、`PriceDraft`、`CursorStack`。
3. [stats_backend.rs](../../gpui/src/stats_backend.rs)：`JobKind`、`JobKey`、
   `StatsScheduler`、`submit_read/write`、`pump_with_reads`、`complete`、`call`。
4. `statistics/` 子模块：[pagination.rs](../../gpui/src/statistics/pagination.rs)、
   [recent.rs](../../gpui/src/statistics/recent.rs)、
   [layout.rs](../../gpui/src/statistics/layout.rs)、
   [import_controls.rs](../../gpui/src/statistics/import_controls.rs)。

## 4. 核心对象关系

```text
GPUI settings process

WindowContent (Entity)
└─ Proxy (Entity, UI-thread owned)
   ├─ cfg: Config
   │  └─ latest trusted saved snapshot
   ├─ drafts: Drafts
   │  └─ editable provider state + optional Pending creation state
   ├─ many input/select/table Entities
   ├─ connections: Connections
   │  └─ explicit Test receipts for this window only
   ├─ backend: Backend
   │  ├─ jobs: SyncSender<Job>
   │  └─ replies: Receiver<Reply>
   └─ statistics: Entity<Statistics>
      ├─ dashboard/rows/groups/recent/prices/detail/settings caches
      ├─ ReplyGate + generations + pagination
      ├─ StatsScheduler
      │  ├─ stats jobs/replies channels
      │  └─ pending read/write intents
      └─ Statistics UI task (250ms or wake)

Backend::new        → settings-control worker   → Core management HTTP
StatsScheduler::new → statistics-control worker → Core management HTTP
workers             → reply channels           → UI tick merges state
```

`cfg`、`drafts`、Entity 和界面 error/status 都归设置进程的 GPUI UI 线程。
两个后台线程只执行已经构造好的命令并把结果送回；它们不直接渲染，也不直接
修改 `Proxy` 或 `Statistics`。Core 管理请求仍先由其网络 runtime 接收、完成
鉴权/解析，再分派具体处理。客户端 worker 不等于 Core 的 HTTP 请求线程。

## 5. 启动与窗口归属

`main::main` 的启动顺序：

1. 解析 `--config-dir`、`--update-progress`、`--version`；
2. 定位配置目录并取得 `gpui.lock`；
3. 已有设置进程持锁时写 `gpui-wake.json` 后退出；
4. 没有 `instance.json` 时提示先启动 Core；
5. 加载 `Config`；
6. 进入 GPUI application，绑定快捷键并打开窗口；
7. `cx.new` 创建 `Proxy`，再创建只持有 Proxy 句柄的 `WindowContent`；
8. 最后一个窗口关闭时退出 application。

`WindowContent` 让不变页面可复用，并把 Kit 的窗口层 modal 留在 sibling
层；`Proxy::new` 则创建页面状态、输入 Entity、表格 Entity、设置 worker、
统计 Entity 和 250ms UI task。

## 6. 供应商操作完整链路

### 6.1 输入到 Draft

用户修改输入框后，事件订阅会调用 `capture`。`capture` 从输入 Entity、协议
设置、effort 和 `models` 投影当前 `Draft`，写入 `drafts.providers`。它不写
`Config`，也不发送 HTTP。`Drafts.persist` 之后才写 `gpui-drafts.json`，而且
`base` 指纹必须与当前 `config.json` 对应，冷启动才不会误用陈旧草稿。

`Drafts.load` 的文件和并发边界：

- 只接受不超过 4 MiB 的草稿文件；
- 无效或旧指纹文件回退为当前配置的干净投影；
- pending 创建记录必须与供应商 ID/created 状态一致；
- 已不存在且不属于 pending 的供应商行会被移除。

### 6.2 点击 Save 到 Core

```text
button / Cmd-S / Ctrl-S
→ Proxy::save
→ save_steps
  → capture current Entity inputs into Draft
  → ordinary provider: Draft::payload("save")
  → pending create: provider_create::request(...)
→ Proxy::send
  → Backend.jobs.try_send(Job { tag, steps })
  → busy = true, status = working, notify

settings-control thread
→ request.recv()
→ backend::execute（backend 模块自由函数）
  → each step in order
  → backend::call_detailed
    → read instance.json port + management_key
    → POST http://127.0.0.1:<port>/_control/action
    → Bearer auth, no proxy, no redirect, bounded body
→ read snapshot/config as appropriate
→ results.send(Reply)

GPUI UI task
→ Proxy::tick
→ Backend.replies.try_recv
→ success/committed/saved merge gate
→ Proxy::update_config
→ update cfg/drafts/connections/form
→ Drafts.persist
→ cx.notify
→ Render
```

`call_detailed` 的普通命令超时为 100 秒，`snapshot/workbuddy_check` 为 3 秒，
回复上限 1 MiB。连接明确未建立或 Core 明确返回客户端错误时，创建流程可以
把失败视为确定；超时、服务端错误、无效响应或无法判断请求是否已执行时，
结果属于不确定。

### 6.3 正常、部分成功和失败边界

```
step1 success + step2 success
→ operation success

step1 save success + step2 test/sync failure
→ execute stops, no remaining step
→ saved_provider preserves “save already landed”
→ UI may merge the saved provider but shows subsequent failure

install/restore/set_port success + restart failure
→ user-visible error
→ synced/committed remains true because disk configuration already changed

save failure
→ ordinary failed save does not enter update_config success path
→ current draft and edit baseline remain

queue full / worker disconnected
→ try_send fails immediately
→ no backend write is claimed
→ UI reports busy and remains usable
```

重启失败不能回滚先前已落盘的配置；界面文案必须保留这个部分成功事实。
`execute` 遇到首个失败会停止后续步骤，不会自动重启或重放整组命令。

### 6.4 外部配置刷新

`Proxy::tick` 每 2 秒节流提交一次 `poll`，并延迟到没有 modal、编辑器或竞争
动作时合并。合并前要求：

- `existing_config` 只读现有文件，不初始化、不迁移、不重写；
- `matches_core` 确认 local key、配置有效性、供应商/模型计数；
- snapshot URL 必须是严格的 loopback `http`，端口匹配，路径 `/v1` 或 `/v1/`，
  且没有用户信息、query 或 fragment。

`refresh_drafts` 会保留仍存在于 after 配置中的脏/正在编辑草稿，以及 pending
创建行。它也会先执行 `retain`：已从 after 消失且不属于 pending 的供应商行
会被删除，即使该行以前有未保存输入。删除当前选择时，选择移动到第一个有效
供应商；若已无供应商则进入空状态。外部刷新完全成功也不发送补偿保存。

`existing_config` 在 `settings-control` worker 中也可能被调用；
`matches_core/refresh_drafts` 才在 GPUI UI 线程完成合并。不能把整个模块说成
只在一条线程运行。

### 6.5 新供应商和复制供应商

新/复制行先得到本地 `draft-...` ID，并写入 `Drafts.pending`。保存时
`provider_create::commit` 执行：

1. 读取 before 配置；
2. 按现有保存语义预演字段和容量，先做校验；
3. 新建发送 `add`，复制发送 `duplicate`；
4. 重新读取 after 配置；
5. 要求相对 before 恰好新增一个供应商，绑定 Core 真实 ID；
6. 把本地临时 model route ID 映射到新 route ID；
7. 对已知真实目标发送最终 `save`。

这不是跨多次 HTTP 请求的原子事务，也没有自动回滚。明确失败可以保留草稿；
超时、重读失败或新增项无法唯一确定时，`Pending.uncertain=true`，不会盲目
重试创建。创建成功但最终保存失败时保留真实 ID 和草稿，下一次只保存已知目标。

### 6.6 连接测试

Test 先构造 save step，再追加 `test`。只要调用前不忙且当前草稿存在，
`Proxy::test` 会在 `send` 后调用 `Connections::begin` 登记 Test Attempt；
该登记不取决于队列是否接收。入队成功时 `busy=true` 并等待 Reply；入队失败时
`busy` 仍为 false，紧接着以 `feedback=None` 调用 `Connections::finish`，
使本次 Attempt 收敛为 Incomplete。Receipt 由 `finish` 生成。Reply 返回后由
当前输入、模型、协议、URL 和报告内容
共同验证。只有管理命令成功、报告 kind 匹配、供应商/模型/协议/endpoint/
reasoning 全部匹配且 HTTP 为 2xx 时才显示 Passed。任何可见字段变化都会使
旧收据隐藏；Core 不可达时空闲 poll 会清除旧收据。它不是持续上游健康监测。

## 7. 统计页完整链路

本节先看界面状态与调度；SQLite 读事务、时间桶、来源聚合、费用扫描和游标边界继续读 [统计查询与界面专题](STATS_QUERY_UI.md)。

### 7.1 从筛选变化到查询

```text
user changes source/range/provider/model/other filters
→ Statistics::tick / sync_filter_state
→ Statistics::mark_dirty
  → generation += 1
  → recent.invalidate
  → reset both pagers and freeze time range
  → rows_gate/groups_gate.invalidate
  → dashboard_seq = 0
  → clear old pending/loading flags for those resources
  → set debounce deadline = now + 250ms
  → notify

debounce expires while page visible and no blocking popover/dialog
→ refresh_report
→ request_dashboard + request_import_status
→ Overview: request_recent
→ Details/Summary: request_rows or request_groups
→ request_health
→ StatsScheduler::submit_read(...)
→ pump_with_reads
→ stats jobs channel
→ statistics-control thread
→ stats_backend::call
→ Core management HTTP
→ StatsReply
→ wake channel
→ Statistics::tick
→ scheduler.complete(key)
→ Statistics::handle_reply
→ identity/generation/tab/seq/tag gate
→ Statistics::apply_reply
→ dashboard/table/pager/error updates
→ cx.notify
```

筛选一改就立即 invalidate，而不是等 250ms 后再失效。因此旧 Reply 在新查询
发出前到达也会被丢弃。`reset_pagination` 同时冻结当前时间窗：在同一个筛选
批次里翻页时，不因每次翻页重新计算 `now`。

### 7.2 Scheduler 的读与写

`JobKey` 比粗粒度 `JobKind` 更具体。scheduler 对读请求的规则是：

- 同一 `JobKey` 只保留最新待发 intent；
- 同资源已有 in-flight 时，新 intent 等待旧 Reply 被 UI `complete`；
- 不同 key 不互相覆盖，例如供应商选项与某个 provider 的模型选项；
- rows/recent/groups/detail 优先于后台资源；
- jobs channel 满时把 intent 放回 pending，下一次 tick 再 pump。

写请求使用最多 32 条的 FIFO backlog。队列满时明确返回错误，不静默丢弃。
离开统计页只取消尚未发出的读请求；已经发给 worker 的读结果无法撤回。返回
后由各资源的 `visibility/tag/seq/generation` 门禁分别处理，不能笼统保证所有
隐藏页面的 Reply 都走同一个忽略分支。写请求保留并继续执行。

### 7.3 去旧结果

页面为不同资源保留不同身份：

| 资源 | 去旧依据 |
| --- | --- |
| Dashboard | `dashboard_seq` |
| Rows | `rows_gate` + `StatsTab::Rows` + scheduler seq |
| Groups | `groups_gate` + `StatsTab::Groups` + scheduler seq |
| Recent | `RecentRead.seq` + filter generation |
| Prices | `prices_seq` |
| Detail | `detail_seq` +返回的 request ID |
| Settings | `settings_seq` |
| Provider/Model options | 各自 `provider_seq/model_seq` |
| Price save/delete/import writes | `PriceDraft.tag/generation` 或独立 write seq |

过旧 Reply 不能修改数据、error、loading 或新请求的 pending 标记。它仍应先
调用 `scheduler.complete(key)` 释放 in-flight；释放资源和接收内容是两个边界。

### 7.4 分页正常与过期

请求明细和分组汇总各自维护 `statistics::pagination::Pager`：

- 第一页 cursor 为 `None`；
- Core 返回的 `next_cursor` 决定“下一页”是否可用；
- 导航按钮调用 `Pager::go(page)` 准备目标页，成功 Reply 的 `finish_page`
  才提交页码变化；
- 请求失败调用 `fail`，可见页保持不变；
- 跳往未缓存页且没有已知 cursor 时，同时发送 page 和当前快照/候选 cursor；
- Core 必须回显所请求 page，否则 UI 报“需要更新并重启后台核心”。

排序或筛选变化会使旧 cursor 失去意义，因此会重置分页器。Core 返回
`cursor_expired` 时，当前实现调用 `reset_pagination`，重置 Rows 与 Groups
两个分页器并重新捕获时间窗，但只立即重取发生错误的列表第一页。

### 7.5 错误与保留数据

`stats_backend::parse_message` 要求成功 envelope 内的 `message` 可再次解析
为 JSON。畸形 JSON 是协议错误，不能降级为空页、空列表或零值。

```text
normal successful reply
→ validate DTO
→ apply data + cursor + timestamps
→ clear relevant error/loading

ordinary dashboard/list query error
→ keep existing visible data
→ show error
→ clear the relevant loading/pending flag only for the current identity

cursor_expired
→ keep the explicit “back to page 1” path
→ reset both pagers and re-capture the time range
→ re-request page 1 for the affected listing

price save error
→ keep dialog, typed values and error
→ do not close a newer draft

maintenance failed
→ preserve history
→ show a stable user-facing error
→ request health again
```

采集健康只来自 `stats_health` 的完整 `HealthDto`。dashboard/rows 查询成功
只证明相应管理响应成功，不能把 Badge 改成“采集正常”。

Recent 有独立的错误策略：当前读取为空或失败时会清掉旧行，显示空态或错误，
避免把旧数据继续称为“最近请求”。不能将上面的普通列表保留策略套到所有子资源。

## 8. 推荐断点和观察点

供应商主线：

- `main::main`：参数、单实例锁、`Proxy::new`、`WindowContent` 创建；
- `Proxy::capture`：输入 Entity 何时投影为 Draft；
- `Proxy::save_steps`：普通保存与 pending 创建请求的分叉；
- `Proxy::send`：Job 进入 channel，还是因队列满被拒绝；
- `backend::execute`：backend 模块自由函数；step 顺序、首次失败停止、
  committed 边界；
- `backend::call_detailed`：backend 模块自由函数；instance、鉴权、超时和
  确定/不确定失败；
- `Proxy::tick`：Reply 合并、部分成功、poll 与 metadata 去旧；
- `Proxy::update_config/reconcile_live_config`：cfg 和 Draft 的最终所有权；
- `provider_create::commit`：真实 ID 绑定、uncertain、保存未完成；
- `provider_connection::finish/view`：Test 收据验证与失效。

统计主线：

- `Statistics::set_visible`：进入/离开页面时哪些资源失效；
- `Statistics::mark_dirty`：generation、冻结时间窗和分页立即失效；
- `Statistics::request_dashboard/rows/groups`：tag、identity 和 seq；
- `StatsScheduler::submit_read/submit_write`：读合并与写排队；
- `StatsScheduler::pump_with_reads`：写优先、channel 满和回填；
- `StatsScheduler::complete`：为什么旧 Reply 也必须释放 in-flight；
- `Statistics::handle_reply`：所有去旧与错误分支；
- `Statistics::apply_reply`：成功数据、cursor 和独立资源合并；
- `statistics::pagination::Pager`：失败路径与当前页状态、跳页 candidate；
- `statistics::recent::RecentRead::apply`：Recent 独立身份。

建议在失败链路上观察 `reply.tag`、scheduler `seq`、filter `generation`、
cursor/page、当前 tab、`loading/pending` 和 `error` 是否属于同一资源。

## 9. 现有测试入口

以下命令定位仓库现有测试模块。修改时按风险选择相关项；纯注释和文档变更
遵循 [L0 验证规范](../CODEX_FAST_WORKFLOW.md)，无需例行编译和运行测试：

```text
cargo test --manifest-path gpui/Cargo.toml --locked state::tests
cargo test --manifest-path gpui/Cargo.toml --locked backend::tests
cargo test --manifest-path gpui/Cargo.toml --locked config_refresh::tests
cargo test --manifest-path gpui/Cargo.toml --locked provider_create::tests
cargo test --manifest-path gpui/Cargo.toml --locked provider_connection::tests
cargo test --manifest-path gpui/Cargo.toml --locked stats_backend::tests
cargo test --manifest-path gpui/Cargo.toml --locked stats_controller::tests
cargo test --manifest-path gpui/Cargo.toml --locked statistics::date_range_tests
cargo test --manifest-path gpui/Cargo.toml --locked statistics::metric_text_tests
cargo test --manifest-path gpui/Cargo.toml --locked statistics::pagination::tests
cargo test --manifest-path gpui/Cargo.toml --locked statistics::recent::tests
cargo test --manifest-path gpui/Cargo.toml --locked ui_contract_tests
```

重点测试覆盖：

- `state::tests`：选择、键盘导航、草稿恢复与过期指纹；
- `backend::tests`：认证 HTTP、重启前提交、部分成功；
- `config_refresh::tests`：只读刷新、脏草稿保留和 after 删除；
- `provider_create::tests`：复制、真实 ID、安全重试和不确定结果；
- `provider_connection::tests`：显式 Test、字段漂移和 Core 丢失；
- `stats_backend::tests`：同 key 合并、写 FIFO、队列满和畸形响应；
- `stats_controller::tests`：generation/seq/tab、价格草稿和 CursorStack；
- `statistics*::tests`：分页、Recent、日期、指标未知值和页面契约。

## 10. 阅读源码时的边界清单

每进入一个重要函数，依次确认：

1. 调用者是按钮、tick、timer、worker，还是测试？
2. 当前在设置 UI、settings worker、stats worker，还是 Core 网络 runtime？
3. 输入来自 Entity、Draft、Config、channel 消息，还是 HTTP JSON？
4. 成功是“已入队”“已发 HTTP”“已保存”“已落盘”中的哪一层？
5. 失败是明确未执行、明确拒绝，还是结果不确定？
6. 谁会消费 Reply；它是否仍是当前 generation/seq/tab/resource？
7. 失败时保留哪份旧数据；哪些 pending/loading 属于新请求，不能清除？
8. 这次修改是否需要 `cx.notify()`，还是只更新后台队列？
9. 外部刷新存在时，当前供应商是否仍在 after 配置中，是否仍在编辑？
10. 写入的是设置进程自己的草稿/DTO，还是 Core 已保存配置和 SQLite？

这十个问题能覆盖本导读涉及的主要所有权、线程、失败和过期响应边界。
