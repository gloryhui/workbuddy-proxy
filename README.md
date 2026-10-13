# WorkBuddy Proxy

专门给 WorkBuddy 使用的轻量模型代理。Rust 转发核心，Windows Win32 / macOS AppKit 原生界面，支持多个供应商和多个模型连接，同一家或不同家、同模型 ID 或不同模型 ID 均可独立列出。支持 OpenAI Chat Completions 与 OpenCode Go。

![Mac 原生设置窗口](docs/native-mac-preview.png)

目标是**更少的常驻内存、更低的转发开销**。本项目不声称在所有平台、所有负载下“全球最小 / 最快”；每次 CI 都构建真实可执行文件、运行冒烟测试并记录可复现的内存与延迟数据。

## 使用

从 [Releases](https://github.com/gloryhui/workbuddy-proxy/releases) 下载与你的系统对应的包。测试与开发中的文件也会出现在 [Actions](https://github.com/gloryhui/workbuddy-proxy/actions) 的构建附件里。

- Windows 10/11 x64：解压，双击 `workbuddy-proxy.exe`，直接显示原生设置窗口。关闭或最小化窗口时隐藏到任务栏托盘，转发继续。单击托盘或再次双击程序可恢复设置；右键托盘“退出”停止程序。Explorer 重启后会重新注册托盘。
- macOS 12+：解压，将 `WorkBuddy Proxy.app` 拖到“应用程序”，双击启动。关闭设置窗口后界面进程退出，转发核心继续运行；再次双击可重新打开设置。
- 无需安装 Rust、Python、Node 或额外 WebView 运行时。

1. 左侧新增一个供应商，只填写一次**供应商名称、OpenAI 兼容地址和 API Key**。支持任意实现 OpenAI Chat Completions 的上游；OpenCode Go 只是其中一家。
2. 在“模型列表”中**每行填写一个真实模型 ID**，也可用 `ID | 显示名称` 自定义列表名称。例如：

   ```text
   deepseek-v4.1-flash | DeepSeek Flash
   deepseek-v4-pro | DeepSeek Pro
   other-model | 我的另一个模型
   ```

3. 保存，点击“测试首个模型”检查当前供应商的首个模型。模型 ID 必须是上游账号实际可用的 ID；不自动猜测、替换或回退。
4. 点击“同步 WorkBuddy 列表”，一次写入所有供应商下已配置的模型，每个模型在 WorkBuddy 中独立显示；需要时重启 WorkBuddy。
5. 修改地址或 Key 即对该供应商全部模型生效，Key 留空保留已保存值。新增模型只需加一行；删除一行后同步，会移除对应 WorkBuddy 项。

最多 64 个供应商、合计 256 个模型，共用一个核心和监听端口。同供应商下重复模型 ID 可填写不同显示名称；不同账号可复制供应商后更换 Key。不同供应商的同名模型有独立别名和选路，不会覆盖或随机选取凭据。

“复制配置”包含当前供应商的本地 URL、连接码和全部模型别名。支持自动读取 `/models` 的客户端可在这个 URL 下获取该供应商的模型列表；WorkBuddy 使用“同步 WorkBuddy 列表”一次写入全部模型即可。复制结果同时列出每个模型的独立 URL，不包含上游 Key。旧版裸别名遇到同名模型时，使用该模型的独立 URL；WorkBuddy 自动同步已使用这些 URL。

DeepSeek V4.1 Flash 会声明思考能力与档位，WorkBuddy 列表可选择“高 / 极致”、允许关闭思考；上下文可选择 128K / 300K / 1M，默认 300K。工具调用默认开启、图片默认关闭；其他模型的能力以真实支持范围为准，必要时在 WorkBuddy 高级配置中调整。

## 同名模型如何区分

每个模型生成一个独立内部 ID，以及带真实模型后缀的 WorkBuddy 别名，例如 `wbp-abc123/deepseek-v4.1-flash`。真实 ID 的后缀使 WorkBuddy 仍可识别模型能力。每个模型还有独立本地 URL `/r/模型ID/v1`，避免客户端处理名称时丢失路由信息。代理将请求的 `model` 还原成所选上游真实 ID，只发送该项的 Key。

| 场景 | 支持 |
| --- | --- |
| 同供应商、同模型 ID、同 Key，作为两项配置 | 是，显示名称不同即可 |
| 同供应商、同模型 ID、不同 Key | 是 |
| 同供应商、不同模型 ID | 是 |
| 不同供应商、同模型 ID | 是 |
| 不同供应商、不同模型 ID | 是 |

`/v1/models` 返回所有已保存供应商的模型别名列表；共用 `/v1/chat/completions` 可按别名路由。若直接发送一个对应多条连接的裸模型 ID，代理返回错误，避免选错供应商。独立模型 URL 支持该项别名或真实模型 ID；供应商 URL `/r/供应商ID/v1` 支持该供应商全部模型，裸 ID 在供应商内多义时也会拒绝。

## 从 v0.1 / v0.2 升级

先从托盘 / 菜单栏“退出”旧版，再运行新版。旧配置备份为私有的 `config.pre-v0.3-*.bak` 后自动迁移；v0.2 中供应商标签、URL、Key 均相同的连接会合并为一个供应商，原模型 ID、别名、思考设置和 WorkBuddy 恢复记录保留，不需要重新填写。不同 Key 会保留为独立供应商。Mac 仅剩旧核心时，新版会识别版本并停止旧核心再启动；旧版窗口仍常驻时会提示先退出。

“恢复配置”只恢复本工具改动的模型。如果该模型后来被其他操作修改，程序拒绝覆盖，并保留备份供手动恢复。备份位于 WorkBuddy 配置旁，名称为 `models.json.workbuddy-proxy-时间戳.bak`。

## 思考强度和 WorkBuddy 选择器

WorkBuddy 的“思考强度”由 `supportsReasoning: true`、`reasoning.defaultEffort` 和 `reasoning.supportedEfforts` 控制；仅填模型 ID / URL / Key 不保证显示。`contextWindow.defaultLength` / `supportedLengths` 控制上下文窗口选择器。“同步 WorkBuddy 列表”会为 `deepseek-v4.1-flash` 写入这些能力字段；已有用户需再次点击并重新打开模型设置，必要时重启 WorkBuddy。

代理的每个模型默认“跟随 WorkBuddy”，保留客户端的思考参数。只有明确选择关闭 / 低 / 高 / 最高并保存，才批量覆盖当前供应商各模型的上游请求，优先级高于 WorkBuddy 当前选择。“保留各模型设置”保留迁移来的不同档位；“测试首个模型”使用该模型已保存的档位。

DeepSeek 的覆盖参数遵循 [官方说明](https://api-docs.deepseek.com/guides/thinking_mode/)：开启时发送 `thinking.type=enabled` 与 `reasoning_effort`；关闭时发送 `thinking.type=disabled` 并删除冲突的 `reasoning_effort`。代理提供 `low / high / max` 参数；WorkBuddy 的 Go 目录目前为 V4.1 声明 `high / max`，所以其列表只展示这两档。其他 OpenAI 兼容模型仅发送 `reasoning_effort`，不保证每家都支持全部档位，上游拒绝时原样返回错误。思考内容和工具历史仍由 WorkBuddy 管理，代理不伪造 `reasoning_content`。

## 轻量设计

| 部分 | 选择 |
| --- | --- |
| 网络核心 | Rust、单线程 Tokio I/O 调度器、Axum / Hyper、Reqwest |
| TLS | 系统原生 TLS 和证书验证 |
| Windows UI | Win32 标准控件和托盘 |
| Mac UI | AppKit 标准控件和菜单栏 |
| 响应 | 流式直通、背压、取消时释放上游流 |
| 请求 | 默认只替换 model 值，其余字节原样保留；真实 ID 无需替换时复用原 Bytes |
| 选路 | 内存哈希索引，模型共享供应商 URL / 凭据，无逐请求目录克隆 |
| 并发 | 4 个在途请求；忙时返回本地 HTTP 429，不排队堆积 |
| 请求体 | 每个最多 8 MiB，输入读取最多 30 秒 |
| 会话缓存 | 最多 64 个会话，每个最多 128 个消息哈希；24 小时不活跃后按需清理 |
| 上游连接 | 每个主机最多保留 2 个空闲连接，60 秒空闲后清理 |

没有内置浏览器、前端框架、数据库、自动更新器、遥测、聊天日志或响应重写。同一个可执行文件包含核心和设置界面：Mac 后台常驻 1 个核心进程，关闭设置会释放界面进程。Windows 为保持托盘，关闭窗口后仍保留核心和一个原生界面进程；需要最低常驻内存时可使用 `--headless`（无托盘）。没有自动模型发现或后台轮询，模型列表由用户明确配置。

端到端自定义请求 / 响应头、错误状态及响应体保留；本地 Authorization / Host 和 hop-by-hop 头不会透传，Authorization 替换为供应商 Key。流式响应保持原始 SSE 字节，不解析、不拼接、不重写。默认不补思考、输出上限或上下文参数。

对于 OpenCode Go，发送本工具自己的 User-Agent，并补齐 `x-opencode-session`。优先读取客户端会话头，缺少时根据聊天历史前缀推断；不同会话不会共用一个全局固定值。相同开场历史的独立聊天可能被归为同一会话；超过缓存窗口、历史压缩和服务重启可能产生新的上游会话。这影响路由 / 缓存，不会混合或交换聊天内容。使用范围与额度以 [OpenCode Go 官方说明](https://opencode.ai/docs/go/) 为准。

## 验证和性能

参见 [设计与性能边界](docs/ARCHITECTURE.md)。基准脚本测量进程 RSS、直接 / 代理的本地请求延迟、4 并发 1 MiB 输入的采样峰值。**本地 HTTP mock 数据不等于真实 TLS、互联网、模型响应或 Windows 人工验收。**

[首版原始性能记录](docs/PERFORMANCE.md)：无界面核心空闲约 Mac 6.23 MiB / Windows 8.61 MiB；这些是无界面核心的数据。Windows 托盘模式还保留 UI 进程，总占用以最新 CI 测量为准。

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --release --locked
python3 scripts/smoke.py --exe target/release/workbuddy-proxy
python3 scripts/multi_routing.py --exe target/release/workbuddy-proxy
python3 scripts/benchmark.py --exe target/release/workbuddy-proxy --output bench-results/local.json
```

Windows 将二进制路径改为 `target/release/workbuddy-proxy.exe`。添加 `--gui` 可测量原生窗口打开时的整个进程。测试脚本只用假 Key 和临时目录，Python 仅为开发 / 验证工具，发布程序不依赖 Python。

## 配置与启动诊断

- Windows：`%APPDATA%\WorkBuddyProxy`
- Mac：`~/Library/Application Support/WorkBuddyProxy`
- Linux（仅无界面开发 / 测试）：`~/.config/workbuddy-proxy`

`config.json` 以本地明文保存上游 Key 和本地连接码。Unix 目录为 0700、配置文件为 0600；Windows 使用当前用户配置目录的默认权限。目录、文件和备份均不要打包分享。只分发官方发布 ZIP，每位使用者填写自己的 Key。

服务仅监听 `127.0.0.1`，校验请求 Host、Origin 和本地 Key；原生界面通过独立管理令牌访问本机控制接口，控制连接不经过系统代理；上游重定向关闭，保留 TLS 验证。订阅 Key 仅发往配置的上游。请求和响应内容不写日志，启动日志最多约 64 KiB；核心与界面分别使用操作系统文件锁。关闭设置不重启核心，也不清空聊天会话缓存。

命令行：`workbuddy-proxy --headless --port 18080 --config-dir <私有目录>`。默认端口冲突会报错，不静默换端口，以免 WorkBuddy 指向错误地址。Windows 包含 `Diagnose.cmd`，原生启动失败也会显示错误框。正常退出会取消在途请求。

上游 429 / 401 / 403 原样返回，不自动重试、换模型或切换计费产品。单次读取上游数据的空闲超时为 180 秒。OpenAI 兼容范围是 `/v1/chat/completions`、多模型 `/v1/models` 和 `/r/模型ID/v1` 独立路由，不包含 Responses、Anthropic 协议转换或 MCP。

Mac 发布包目前仅 ad-hoc 签名，未做 Developer ID 公证；Windows 文件未做 Authenticode 签名。系统可能提示未知开发者。没有自动移除隔离属性或关闭安全保护的脚本。

## 开发与许可

构建需要 Rust 1.88+ 和平台 C 编译器。Mac 使用 Xcode 命令行工具，Windows 使用 MSVC Build Tools。CI 使用 Cargo.lock 锁定依赖，Windows 静态链接 CRT。

MIT License。第三方项目，与 WorkBuddy、腾讯、OpenCode 无隶属关系。
