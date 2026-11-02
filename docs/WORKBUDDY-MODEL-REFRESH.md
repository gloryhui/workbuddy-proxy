# WorkBuddy 模型同步与重启

适用于 v0.6.1。模型同步写入配置后，WorkBuddy 可能需要重新加载进程内模型列表。

## 原因与确认

用户在同一个 WorkBuddy 会话中从 OpenCode Go 的 DeepSeek 切换到幻灵 GPT 后，看到“当前自定义模型未配置接口地址或 API key”。只读检查发现本地模型文件的地址与 Key 已配置，旧会话却在解析新模型端点时抛出 `custom_model_config` / 4100，请求没有到达代理。用户完全退出并重新打开 WorkBuddy 后确认原会话恢复正常。

这是 WorkBuddy 原会话配置刷新问题；此次故障没有证据指向跨供应商聊天历史转换或上游 API 错误。代理侧的调整不承诺让 WorkBuddy 的旧会话自动热更新。

## 用户操作

点击右侧“同步 WorkBuddy”只打开选择弹窗，按钮按顺序为：

1. **同步并重启**：保存当前供应商草稿 → 核心 `install` 同步模型文件 → 请求正常退出 WorkBuddy → 等待原进程退出 → 打开原应用。
2. **仅同步**：执行同一保存和同步步骤，保留 WorkBuddy 进程。
3. **取消**：不保存、不同步、不操作 WorkBuddy。关闭弹窗或 Esc 同样取消。

弹窗说明重启会中断 WorkBuddy 当前任务，请先保存工作。macOS 以外的平台禁用“同步并重启”并解释手动重开方式。重启选择不持久化，每次同步都由用户明确选择。

## 实现边界

- 核心 `configure_workbuddy` 比较修改前后的 JSON 值。语义相同时不备份、不改写 `models.json`，保留外部编辑器的格式、mtime 与 inode；继续更新本工具的安装恢复记录。
- 实际发生变化时继续先备份，再原子替换；没有通过截断写入来规避 WorkBuddy 的文件监听问题。
- GPUI 的 `sync_open` 使用现有模态窗口机制。只有点击两个同步选项才计算草稿保存步骤，取消不会产生控制请求。
- 后台作业标签 `install_restart` 仅用于 GPUI 本地工作线程，不增加核心远程管理命令。每个保存步骤和最终 `install` 必须成功才能重启，且只有最终步骤是 `install` 才允许执行。
- 重启失败时返回“模型已同步到文件”及失败阶段；`Reply.synced` 让界面仍刷新已经保存的配置，不把重启失败当成保存失败。
- macOS 原生实现固定匹配 bundle ID `com.tencent.workbuddy.mac`，使用 AppKit 正常退出接口。先确定并验证重开应用的 bundle URL；多实例直接停止，不按进程名终止、不调用强制退出、不执行 shell 或 AppleScript。
- 等待退出最多 20 秒；拒绝退出、未处理的保存弹窗或超时都会停止，不发起第二个实例。退出完成后通过 NSWorkspace 打开同一 bundle URL，不要求应用固定安装在 `/Applications`。
- 启动结果等待最多 15 秒；启动请求未确认时提醒用户检查应用状态，不宣称一定没有启动。应用原本未运行时尝试直接打开；找不到应用时提示手动打开。
- AppKit 调用在主队列，等待过程在设置后台线程，界面线程不轮询阻塞。

## 验证边界

历史调用、隔离 fixture 和平台验证记录见 [固定版本原始报告](https://github.com/gloryhui/workbuddy-proxy/blob/v0.6.1/docs/WORKBUDDY-MODEL-REFRESH.md)。模拟应用的退出与重启结果不能代替任意真实 WorkBuddy 安装环境验收；变更该链路时按 [风险规范](CODEX_FAST_WORKFLOW.md) 核对实际进程和配置保护。
