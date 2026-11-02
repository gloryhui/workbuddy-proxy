# 桌面应用 · v0.6.1

WorkBuddy Proxy 使用 Rust Core、原生托盘和 GPUI 设置界面。设置窗口按需启动；关闭窗口会释放 GPUI 进程，Core 继续转发和采集，从托盘可再次打开。退出应用须使用托盘/菜单栏的退出操作。

## 设置界面

- 供应商管理包含连接、协议、模型目录、能力编辑、测试、保存与 WorkBuddy 同步。编辑草稿和保存配置分开处理，外部配置变更不得覆盖用户正在编辑的内容。
- 数据统计包含概览、请求明细和汇总分析；统计设置管理采集、价格、存储及可选导入。具体口径见 [统计说明](statistics/README.md)。
- 中英文本地化、皮肤、开机启动、检查更新、恢复窗口尺寸、GitHub 与反馈使用共享原生工具栏。弹层跟随各自按钮，窗口按钮保留系统原生行为。
- 提供黑白、蓝白、紫藤、玫瑰、琥珀、青绿与 Studio Indigo 主题，外观独立保存。字体使用系统字体和 fallback，不嵌入完整字体包。
- 模型/发现列表使用虚拟化，日志有界。没有浏览器、WebView、JavaScript 运行时或完整图标目录。

模型能力、协议与资源边界分别见 [模型能力](MODEL-CAPABILITIES.md)、[上游协议](protocol-compat/USAGE.md) 与 [设计取舍](ARCHITECTURE.md)。

## 本地数据

完整配置目录包含供应商配置、SQLite 用量库、私有草稿、外观与统计筛选偏好。配置和 `gpui-drafts.json` 可能含 API Key，不能作为公开截图、日志或 Issue 附件。配置目录可由 `--config-dir` 或 `WORKBUDDY_PROXY_CONFIG_DIR` 指定，检查实际路径后再备份。

默认 Windows 为 `%APPDATA%\WorkBuddyProxy`，macOS 为 `~/Library/Application Support/WorkBuddyProxy`。升级与恢复时完整保留 SQLite 及存在的 WAL/SHM，不只复制主文件。见 [备份与冷回滚](studio_v2/UPGRADE_ROLLBACK.md)。

开机启动默认关闭，仅注册当前用户；失败会回退保存结果。自启动运行托盘和 Core，不自动弹出设置，也不在用户主动退出后强制重启。

## Windows 安装与卸载

优先使用正式 Release 的 x64 setup，便携 ZIP 保持原目录结构。安装器使用简体中文，默认安装到 `%LOCALAPPDATA%\Programs\WorkBuddy Proxy`，快捷方式只启动 `workbuddy-proxy.exe`；`internal/workbuddy-settings.exe` 不可单独移动或启动。

卸载会退出属于该安装目录的程序并移除安装文件、internal 和快捷方式，保留外部用户配置与统计数据。同名的其他安装实例不作为退出目标。受保护目录需要相应写入权限，程序不自动提权。Inno Setup 是构建依赖，运行无需安装 Rust 或 Python。

## 更新与验证

检查更新弹窗展示正式 Release 信息，提供确认下载、前往 GitHub 和关闭操作；下载完成后再次确认重启安装。已是最新版时保留“前往 GitHub / 完成”。完整流程、日志及失败恢复见 [软件更新](AUTO-UPDATE.md)。

当前发行范围及未验证项见 [v0.6.1 发布说明](RELEASE_v0.6.1.md)。自动化、模拟控制器或截图结果不能代替真实供应商、辅助技术、输入法和各平台实机专项。
