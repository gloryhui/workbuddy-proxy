# 多协议架构与配置契约

## 已核实的现有调用链

调研基线 `e1725c3d14c6dfed24cd4edc0150c1c052ab4bed` 尚无统计实现，以下为已核实符号入口，不依赖行号。正式开发必须先按PARALLEL-WORK.md从统计合入后的新基线创建worktree，并补读实际统计代码，不能将本表当成合入后的完整调用链。

| 现有位置 | 当前职责 | 必须接入的变化 |
| --- | --- | --- |
| `src/config.rs` Provider/Connection、connections/load/group/validate | 存储、旧版迁移、扁平连接、endpoint | 新协议配置与默认、传播、严格验证、旧数据恢复 |
| `src/proxy.rs` Routes::new/resolve、Destination/Route | 预解析目标和 Bearer，快照选路 | 原生鉴权/协议快照/模型相关 endpoint |
| `src/proxy.rs` relay_to/upstream | body、改 model/思考、reqwest、原流返回 | 明确 fast path 与 adapter path，统一持有 permit |
| `src/proxy.rs` Bridge::test | 独立组请求并解析 choices 行 | 共用转换引擎，不新增假测试路径 |
| `src/proxy.rs` discover/metadata_targets/command_inner | 获取模型、元数据、保存/复制/删除 | 协议字段所有保存入口与 discovery 接入 |
| `src/catalog.rs` Target/refresh/fetch_provider/endpoint | 默认 Bearer + `/models` + data[] | 原生列表、缓存身份、分页与能力来源分离 |
| `src/capabilities.rs` ModelCapabilities::apply | WorkBuddy 能力和 custom URL | 适配器能力取交集，useCustomProtocol 含义保持 |
| `src/workbuddy.rs` entry/configure/restore | 配置同步和恢复 | 继续写本地 Chat 地址，不写上游原生地址 |
| `src/diagnostic.rs` TestReport | 结构化诊断、中英文 | 增加实际协议与脱敏目标；不存 body/headers |
| `gpui/src/state.rs` Draft/Drafts、payload | 草稿持久化与保存 payload | 协议和设置新字段全链路保留 |
| `gpui/src/ui.rs` supply/preset/save/fetch/test | 供应商编辑、异步 job、语言 | 协议下拉、相关设置、不同协议模型发现 |
| `gpui/src/presets.rs`、`assets/provider-presets.json` | 现有 76 个 Chat 地址预设 | 老预设明确默认 Chat，不能按厂商强改 |
| `src/main.rs` CONTROL_PROTOCOL、`src/frontend.rs` | 核心/UI兼容和唤醒 | 有不兼容新增协议配置时升级并防旧 UI 覆盖 |
| `gpui/src/main.rs` path imports | 直接复用核心 config/capabilities 等文件 | 共享纯 DTO，不能引入 reqwest/Tokio 到 UI |

当前 `Connection::endpoint()` 固定 Chat 路径。`Destination` 的 URL 和认证按供应商共享；Gemini URL 含模型，不能把第一个模型 URL 共享给所有模型。保留供应商共享凭据和协议快照，每个 Route 预生成自己的模型目标。

当前 catalog 的 canonical endpoint 去掉版本/部分路径，不能直接用于网络请求，也不能误把不同协议的实时列表缓存混为一条。公开 models.dev 可共享静态知识；实时供应商列表必须有协议身份。

## 建议文件分工

```text
src/protocol/
  mod.rs          # Protocol 枚举、选路接口、转换入口，纯配置另拆 types.rs
  types.rs        # serde/std/serde_json 配置 DTO，核心和 GPUI 可共享
  request.rs      # Chat 输入校验和内部语义表示
  event.rs        # 统一内部输出事件；绝不存整个聊天会话
  chat.rs         # 显式透传策略，保留旧原字节实现
  responses.rs    # Responses 请求/JSON/事件解析
  anthropic.rs    # Messages 请求/JSON/事件解析
  gemini.rs       # GenerateContent 请求/JSON/SSE解析
  sse.rs          # 有界、跨chunk SSE字节帧
  encode.rs       # Chat JSON和SSE输出
  errors.rs       # 安全错误分类
  replay.rs       # 仅内存的签名/opaque工具循环缓存
  discovery.rs    # 协议原生模型列表解析和有界分页
scripts/protocol_compat_test.py
scripts/protocol_compat_benchmark.py
tests/fixtures/protocol_compat/  # 脱敏、离线的输入与golden输出
```

允许合并少量过短文件或依据现有 Rust 结构调整；职责必须可定位。不能把三个适配器、SSE状态和UI逻辑全部堆到 `proxy.rs`。共享 DTO 不依赖网络库；注意 gpui 将 url crate 别名为 reqwest，只提供 URL 类型，不是核心 reqwest HTTP client。

## 配置形状

新增字段都放供应商层。模型仍保留现有 id/alias/model/name/capabilities/reasoning_effort，不在模型层复制 Key。建议形状如下，最终字段名采用本表，不另外发明第二套命名：

```json
{
  "protocol": "anthropic_messages",
  "protocol_settings": {
    "anthropic_version": "2023-06-01",
    "default_max_output_tokens": 4096,
    "reasoning_profile": {
      "mode": "none",
      "budgets": {},
      "levels": {}
    },
    "responses_efforts": {}
  }
}
```

| 字段 | 约束 |
| --- | --- |
| protocol | enum 四个值；字段缺失默认 openai_chat；未知值/null/数字拒绝，不回退 |
| protocol_settings | 缺失用默认；未知键拒绝；不存任意header模板/任意原生请求JSON |
| anthropic_version | 当前仅接受已审定2023-06-01，缺失用默认；不开放任意beta header |
| default_max_output_tokens | u32正数，默认4096，仅请求未给输出上限且原生协议要求时使用；不得覆盖显式客户端值 |
| reasoning_profile.mode | none / anthropic_budget / anthropic_adaptive / gemini_budget / gemini_level；协议不匹配拒绝 |
| reasoning_profile.budgets | budget型按 low/medium/high/xhigh/max 显式给正整数；未配置档位不得猜数值；不支持的组合明确报错 |

2026-10-16新增`reasoning_profile.levels`：effort→native level的字符串映射，Anthropic允许low/medium/high/max，Gemini允许minimal/low/medium/high；只在相应模式使用。新增`responses_efforts`：本地effort→Responses effort的字符串映射（none/minimal/low/medium/high/xhigh），仅Responses可配置；例如`{"max":"xhigh"}`。缺少映射的本地max明确拒绝，其他未核实模型档位不声称全支持。两者均为封闭字段，不是任意原生JSON。只允许 effort→native effort/level 的显式映射，不增加任意JSON透传。未设置原生思考 profile 时可以依赖上游默认思考，但不能声称接受所有客户端 effort 覆盖。Responses effort 使用已核实的原生值映射与模型支持集；不能把 max 硬映为任意值。

`reasoning_effort=default` 保留当前“跟随 WorkBuddy”含义，keep 是编辑批次指令而非发送上游值。none 映射为显式关闭时先验证模型允许。预算不合法、预算与 max_tokens 冲突时 400；不得静默增大预算/上限改变成本。

任何新增字段都必须贯穿 Provider → Connection → 路由快照 → 保存/复制/草稿 → 发现/测试/同步能力。检查旧版 migration grouping，协议和有效设置属于分组身份。已有 ID、alias、installation.expected/previous、local_key、端口不变。

旧命令省略协议字段时必须保留当前供应商协议；新建供应商缺失时才默认 Chat。正式开始时读取统计合入后的真实CONTROL_PROTOCOL，有不兼容变化则再递增，并修正对应兼容fixture。不能硬编码本次必须用5；统计可能已经使用5，同号不代表不同UI/core彼此兼容。详见PARALLEL-WORK.md。

写配置沿用原子写和私有目录；协议或设置非法时整个保存失败，不能先清空models再报错。修改协议保留模型行和Key/地址草稿，由校验指出URL不匹配；不自动删模型或覆盖Key。

## URL 与认证

| protocol | 用户填写 base 示例 | 对话目标 | 认证 |
| --- | --- | --- | --- |
| openai_chat | `https://host/gateway/v1` | `/gateway/v1/chat/completions` | Bearer Key |
| openai_responses | `https://host/gateway/v1` | `/gateway/v1/responses` | Bearer Key |
| anthropic_messages | `https://host/gateway/v1` | `/gateway/v1/messages` | x-api-key + anthropic-version |
| gemini_generate_content | `https://host/gateway/v1beta` | `/gateway/v1beta/models/{model}:generateContent` 或 `:streamGenerateContent?alt=sse` | x-goog-api-key |

1. Chat endpoint 兼容原有完整 `/chat/completions` 输入。Responses/Messages 可接受匹配协议的完整端点并规范化为base。其他协议端点混填时保存失败并给出修正示例，不自动猜协议。
2. Gemini仅接收base，允许模型ID输入 `models/foo` 或 `foo`，规范化一次；不接受用户填写完整generateContent目标，UI提示base示例。
3. 保留所有用户网关前缀和端口，不强行重建 `/v1`。使用 URL API，不让以 `/` 开头的 join 抹去网关前缀。
4. Gemini模型ID作为受校验单个路径段；拒绝 `/`（已剥离的models前缀除外）、`..`、`?`、`#`、控制字符、编码斜杠/双重编码逃逸；支持当前正式API合法的连字符/点等字符。不把本地可读别名放进原生模型URL。
5. 保持HTTPS和loopback HTTP限制，base不得含query、fragment、userinfo。跨协议不转发客户端query；非空query先400；Gemini alt=sse 仅由代理产生。Chat原query行为保持。
6. 原生请求headers白名单：Content-Type/Accept、所选协议认证和明确兼容的request-id。禁止透传客户端Authorization、x-api-key、x-goog-api-key、Cookie、Proxy-Authorization及任意原生beta headers来覆盖路由Key。
7. Chat保持已验证的端到端header规则，但额外审计原生凭据header是否可能泄漏；如需收紧，只删除明确凭据header并补回归，不动普通未知header。
8. 不把原生鉴权header、Key、完整错误或消息写诊断。新日志只记录协议、路由标识、状态、安全错误码和脱敏目标。

## 请求处理与输出事件

入口只做一次本地认证/路由/许可，调用统一 PreparedRequest。Chat fast path 调用旧 rewrite_request，原流直出。原生 path：Chat校验 → 显式覆盖思考 → 语义转换 → 原生请求 → 语义事件 → Chat编码。不要先把Chat思考JSON规则写到原生body再修补。

内部事件至少区分 Start、TextDelta、ReasoningDelta、ToolStart、ToolArgumentsDelta、ToolEnd、Usage、Finish、Failure。每个流拥有状态机和tool index表。适配器输出事件必须被公共编码器验证，不能各自随便拼SSE。非流式可从相同语义对象编码，不要求先人为造SSE再解析。

Bridge::test 使用相同 PreparedRequest 和响应适配器，再消费Chat事件。避免通过本地HTTP回调自己重复鉴权/占用第二个permit。诊断目标URL应为实际原生URL，添加protocol；不展示Key/query。

## 资源硬预算

下表为新转换链的工程限制，执行者不能只写注释，必须有溢出错误和测试。MiB=1024×1024。

| 资源 | 上限与行为 |
| --- | --- |
| 入口 body | 8 MiB，沿用原先30秒读取期限 |
| 原生非流式 body | 16 MiB，先限长再反序列化，超限502 |
| 单个SSE帧含data | 1 MiB，超限终止该转换流，不无限等待分隔符 |
| SSE待解析驻留 | 1 MiB + 64 KiB，分片处理任意大的网络chunk，不依赖chunk自然较小 |
| 单个工具arguments | 256 KiB UTF-8，不能截断成合法JSON冒充完整 |
| 单响应原生解析聚合状态 | 16 MiB总量，文本/思考/签名均计入；须为潜在工具回合保留原生assistant块，到普通回答终态不写全局缓存 |
| 单响应tool calls | 64，超限明确失败；可见delta按pull增量发送；用于完整工具回放的原生assistant blocks暂存受16 MiB聚合预算约束 |
| 单响应opaque回放 | 8 MiB，单项1 MiB；预算包含签名及必须完整保留的原生block |
| 进程replay缓存 | 32 MiB、最多512项、30分钟TTL，以较早达到者为准，LRU淘汰 |
| 下游输出待发队列 | 最多16事件、合计1 MiB，或等价pull式单事件发送；超大单事件拆文本delta |
| 模型发现 | 合计1 MiB原始响应预算、4096模型、最多16页、总30秒deadline；循环游标拒绝 |

允许依据实测提出提高某项预算，但必须记录改动理由、溢出fixture和RSS结果，不得去掉限额。不要通过无限的unbounded_channel、Vec、HashMap回避上限。OOM不是协议错误处理方式。

沿用15秒连接timeout、180秒读取timeout和连接池策略。SSE parser只按需拉上游，不fork后台reader。Drop释放reqwest流、permit和未完成replay记录。流间状态不放全局锁内等待网络。不得锁住config/routes等待await。

replay仅存恢复原生签名/工具内容所必需的opaque块与绑定信息，内存不落盘、不进统计/日志/导出。不得缓存完整用户history、图片或普通回答以简化实现。签名块本身可能包含思考原文，因此生命周期和字节预算都必须覆盖原文。

## 模型发现与能力

- Chat/Responses使用base下GET models，Bearer；Messages使用原生models列表和版本header；Gemini用原生models与pageToken，读取明确支持生成的方法。具体分页shape由官方fixture确定。
- 所有发现复用保存前draft的protocol/base/key，不误读保存前旧协议。不支持models的供应商允许手填，并清楚显示发现失败，不能退回其他协议重试。
- display name/id规范化与现有模型选择窗口一致，去重按真实ID，不取代现有显示名/手动项。
- 实时缓存键至少protocol+规范base+凭据指纹；凭据使用现有无泄漏策略或进程内generation，不将可离线猜测的Key哈希写入日志。切换Key不复用前账号的“已授权列表”。
- 跨协议公开能力不可等价于已适配能力；models.dev不是账号授权或协议支持证明。来源/手工设置保留，计算独立effective capability供同步与UI使用。
- 现有76个预设都是Chat输入，新增协议字段默认Chat；不批量把Claude品牌改成Messages或Google改成Gemini。新增原生预设必须有官方来源，数量文案随之正确更新，否则本次只提供手动协议选择。

## GPUI 与可观察性

供应商连接区增加“上游协议 / Upstream protocol”必选下拉，四种名称明确。切换后URL提示、发现和思考配置随协议更新；显示仅原生需要的版本/默认上限/profile设置，不向WorkBuddy暴露这些实现细节。

协议选择必须跟随选中供应商、复制、未保存草稿、窗口关闭重开、语言切换、保存失败保持输入、保存后刷新。不允许render每帧覆盖用户选择；Job返回旧provider时不能把结果写到新选中的provider。

新控件具备accessibility label、键盘操作和中英文；1100×700、1180×780有完整可用布局。测试成功信息要说明“连接探测通过”，不能写“全部工具/图片兼容已验证”。错误通过安全code与字段路径定位，不展示原始上游body。

只改上述必要文件和本任务新增目录。需要改CI让新脚本真正运行；不得删除旧检查、扩大Actions权限、触发真实Key测试或自动发布。
