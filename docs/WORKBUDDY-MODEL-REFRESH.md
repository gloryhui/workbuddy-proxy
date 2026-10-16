# WorkBuddy 模型同步与重启

日期：2026-10-16。基线：`e4809f3`。版本仍为 0.5.3，核心管理协议不变。

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

## 验证

- 核心单测 175 项：包括重复同步保持字节、mtime、Unix inode、备份数量，以及不同格式的等价 JSON 保持原样。
- GPUI 单测 63 项：包括保存失败、同步失败均禁止重启；仅同步不重启；缺少同步步骤不重启；同步成功而重启失败仍报告已落盘。
- 两个 Cargo 工程 `fmt --check`、严格 clippy 与本地 Debug 构建。
- `model_capabilities_test.py`、`multi_routing.py`：隔离 HOME、临时配置、loopback fixtures 验证同步/恢复、元信息、供应商隔离与别名选路。
- `workbuddy_restart_test.py`：编译真实原生实现与临时 Cocoa 应用，验证缺失应用、正常退出并以新 PID 重开、拒绝退出后超时且不强杀/不重复启动、多实例不处理。仅 fixture 把退出等待缩为 1 秒，正式路径固定 20 秒。运行结束正常退出并清理 fixture，不操作真实 WorkBuddy。

复现：

```sh
cargo test --quiet
cargo clippy --all-targets -- -D warnings
cargo build --locked
cargo fmt --check
cargo test --manifest-path gpui/Cargo.toml --quiet
cargo clippy --manifest-path gpui/Cargo.toml --all-targets -- -D warnings
cargo build --manifest-path gpui/Cargo.toml --locked
cargo fmt --manifest-path gpui/Cargo.toml --check
python3 scripts/model_capabilities_test.py --exe target/debug/workbuddy-proxy
python3 scripts/multi_routing.py --exe target/debug/workbuddy-proxy
python3 scripts/workbuddy_restart_test.py
```

实际 WorkBuddy 自动重启及弹窗的人工验收未执行；GUI 检查时 Mac 已锁定，停止了隔离测试窗口，没有要求用户解锁或重启其应用。用户确认的是手动重启解决原会话配置问题。没有发布、安装或替换已有测试 DMG。

补充探测发现临时目录中的 fixture 完全退出后，LaunchServices 无法按 bundle ID 重新定位它，原生实现正确返回“未找到应用”。因此未用该临时 fixture 宣称验证了“已安装且未运行的 WorkBuddy”自动打开；这仍需要实际安装环境验收。主工作区另有尚未提交的价格功能改动，本批留在独立开发分支，未覆盖或合并那些工作。
