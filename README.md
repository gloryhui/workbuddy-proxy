# WorkBuddy Proxy

专门给 WorkBuddy 使用的轻量模型代理。Rust 转发核心，Windows Win32 / macOS AppKit 原生界面，支持多个供应商和多个模型连接，同一家或不同家、同模型 ID 或不同模型 ID 均可独立列出。支持 OpenAI Chat Completions 与 OpenCode Go。

![Mac 原生设置窗口](docs/native-mac-preview.png)

目标是**更少的常驻内存、更低的转发开销**。本项目不声称在所有平台、所有负载下“全球最小 / 最快”；每次 CI 都构建真实可执行文件、运行冒烟测试并记录可复现的内存与延迟数据。

## 使用

从 [Releases](https://github.com/gloryhui/workbuddy-proxy/releases) 下载与你的系统对应的包。测试与开发中的文件也会出现在 [Actions](https://github.com/gloryhui/workbuddy-proxy/actions) 的构建附件里。

- Windows 10/11 x64：解压，双击 `workbuddy-proxy.exe`，直接显示原生设置窗口。关闭或最小化窗口时隐藏到任务栏托盘，转发继续。单击托盘或再次双击程序可恢复设置；右键托盘“退出”停止程序。Explorer 重启后会重新注册托盘。
- macOS 12+：解压，将 `WorkBuddy Proxy.app` 拖到“应用程序”，双击启动。关闭设置窗口后界面进程退出，转发核心继续运行；再次双击可重新打开设置。
- 无需安装 Rust、Python、Node 或额外 WebView 运行时。

1. 左侧选择连接，填写**列表显示名称、供应商、上游地址、自己的 API Key 和真实模型 ID**，保存并测试。默认 OpenCode Go 地址为 `https://opencode.ai/zen/go/v1`，模型为 `deepseek-v4.1-flash`。
2. 点击“新增”配置另一条；“复制”可复制当前连接的地址、Key 和模型，再调整显示名称、供应商或模型。每项的名称需要不同；Key 留空只保留当前项已保存的 Key。
3. 点击“同步 WorkBuddy 列表”。程序先备份 `models.json`，一次同步所有已保存 Key 的连接，保留其他模型。
4. 重新打开 WorkBuddy 模型列表，必要时重启客户端。每个连接显示自己的名称、供应商和模型；WorkBuddy 会额外附带模型别名用于区分，分别选择即可。
5. 删除代理连接后再次同步，会移除对应 WorkBuddy 项。修改上游模型 ID 后同步，会清理旧列表项。没有 Key 的新连接是草稿，不会同步。

最多 64 个连接，共用一个转发核心、一个监听端口和一个原生窗口。**供应商、URL、Key、真实模型 ID 都可以重复**，只需列表显示名称不同。不同连接不会因为模型同名而覆盖、随机选路或自动切换 Key。

也可点击“复制配置”，手动添加当前项。它包含独立连接 URL、本地连接码和代理生成的模型别名，三者配套使用。不要把别名手工改回真实模型 ID、也不要把上游订阅 Key 填到本地代理连接中。

DeepSeek V4.1 Flash 会声明思考能力与档位，WorkBuddy 列表可选择“高 / 极致”、允许关闭思考；上下文可选择 128K / 300K / 1M，默认 300K。工具调用默认开启、图片默认关闭；其他模型的能力以真实支持范围为准，必要时在 WorkBuddy 高级配置中调整。

## 同名模型如何区分

每条连接生成一个独立内部 ID，以及带真实模型后缀的 WorkBuddy 别名，例如 `wbp-abc123/deepseek-v4.1-flash`。真实 ID 的后缀使 WorkBuddy 仍可识别模型能力。每项还有独立本地 URL `/r/连接ID/v1`，避免客户端处理名称时丢失路由信息。代理将请求的 `model` 还原成所选上游真实 ID，只发送该项的 Key。

| 场景 | 支持 |
| --- | --- |
| 同供应商、同模型 ID、同 Key，作为两项配置 | 是，显示名称不同即可 |
| 同供应商、同模型 ID、不同 Key | 是 |
| 同供应商、不同模型 ID | 是 |
| 不同供应商、同模型 ID | 是 |
| 不同供应商、不同模型 ID | 是 |

`/v1/models` 返回所有已保存连接的别名列表；共用 `/v1/chat/completions` 可按别名路由。若直接发送一个对应多条连接的裸模型 ID，代理返回错误，避免选错供应商。独立连接 URL 支持该项别名或真实模型 ID。

## 从 v0.1 升级

先从托盘 / 菜单栏“退出”旧版，再运行新版。旧的单连接配置会备份为私有的 `config.pre-v0.2-*.bak` 后迁移，保留上游 Key、本地连接码、端口和恢复记录；已存在的裸模型标识继续可用。Mac 旧设置已关闭、只剩旧核心时，新版会识别版本并先停止旧核心再启动新核心。旧版窗口仍常驻时，新版会提示退出旧版，避免把新设置交给旧核心。

“恢复配置”只恢复本工具改动的模型。如果该模型后来被其他操作修改，程序拒绝覆盖，并保留备份供手动恢复。备份位于 WorkBuddy 配置旁，名称为 `models.json.workbuddy-proxy-时间戳.bak`。

## 思考强度和 WorkBuddy 选择器

WorkBuddy 的“思考强度”由 `supportsReasoning: true`、`reasoning.defaultEffort` 和 `reasoning.supportedEfforts` 控制；仅填模型 ID / URL / Key 不保证显示。`contextWindow.defaultLength` / `supportedLengths` 控制上下文窗口选择器。“同步 WorkBuddy 列表”会为 `deepseek-v4.1-flash` 写入这些能力字段；已有用户需再次点击并重新打开模型设置，必要时重启 WorkBuddy。

代理的每条连接默认“跟随 WorkBuddy”，保留客户端的思考参数。只有该项明确选择关闭 / 低 / 高 / 最高并保存，才覆盖该连接的上游请求，优先级高于 WorkBuddy 当前选择。“测试”也使用已保存的同一档位。

DeepSeek 的覆盖参数遵循 [官方说明](https://api-docs.deepseek.com/guides/thinking_mode/)：开启时发送 `thinking.type=enabled` 与 `reasoning_effort`；关闭时发送 `thinking.type=disabled` 并删除冲突的 `reasoning_effort`。代理提供 `low / high / max` 参数；WorkBuddy 的 Go 目录目前为 V4.1 声明 `high / max`，所以其列表只展示这两档。其他 OpenAI 兼容模型仅发送 `reasoning_effort`，不保证每家都支持全部档位，上游拒绝时原样返回错误。思考内容和工具历史仍由 WorkBuddy 管理，代理不伪造 `reasoning_content`。

## 轻量设计

| 部分 | 选择 |
| --- | --- |
| 网络核心 | Rust、单线程 Tokio I/O 调度器、Axum / Hyper、Reqwest |
| TLS | 系统原生 TLS 和证书验证 |
| Windows UI | Win32 标准控件和托盘 |
| Mac UI | AppKit 标准控件和菜单栏 |
| 响应 | 流式直通、背压、取消时释放上游流 |
| 请求 | 只还原顶层模型别名和显式思考设置；消息 / 工具保持 RawValue 原始 JSON |
| 并发 | 4 个在途请求；忙时返回本地 HTTP 429，不排队堆积 |
| 请求体 | 每个最多 8 MiB，输入读取最多 30 秒 |
| 会话缓存 | 最多 64 个会话，每个最多 128 个消息哈希；24 小时不活跃后按需清理 |
| 上游连接 | 每个主机最多保留 2 个空闲连接，60 秒空闲后清理 |

没有内置浏览器、前端框架、数据库、自动更新器、遥测、聊天日志或响应重写。同一个可执行文件包含核心和设置界面：Mac 后台常驻 1 个核心进程，关闭设置会释放界面进程。Windows 为保持托盘，关闭窗口后仍保留核心和一个原生界面进程；需要最低常驻内存时可使用 `--headless`（无托盘）。运行时不进行模型目录同步和后台余额轮询。

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

上游 429 / 401 / 403 原样返回，不自动重试、换模型或切换计费产品。单次读取上游数据的空闲超时为 180 秒。API 覆盖 `/v1/chat/completions`、多模型 `/v1/models` 和 `/r/连接ID/v1` 独立路由，不包含 Responses、Anthropic 协议转换或 MCP。

Mac 发布包目前仅 ad-hoc 签名，未做 Developer ID 公证；Windows 文件未做 Authenticode 签名。系统可能提示未知开发者。没有自动移除隔离属性或关闭安全保护的脚本。

## 开发与许可

构建需要 Rust 1.88+ 和平台 C 编译器。Mac 使用 Xcode 命令行工具，Windows 使用 MSVC Build Tools。CI 使用 Cargo.lock 锁定依赖，Windows 静态链接 CRT。

MIT License。第三方项目，与 WorkBuddy、腾讯、OpenCode 无隶属关系。
