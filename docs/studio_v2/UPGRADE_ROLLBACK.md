# Studio V2 升级备份与冷回滚

> 发布准备文档。候选运行时代码为 `9570451`，仍使用测试版本字符串 `0.5.7`，与公开 v0.5.7 包内容不同，必须按来源与 SHA256 区分。正式版本、签名信任及全部发布门禁尚待签收，本页不构成发布声明。

## 升级前保留什么

1. 从托盘或菜单栏退出，等待设置窗口、宿主与 Core 全部退出。仅关闭设置窗口会保留转发 Core。停止客户端的新请求，退出后再复制；不要在运行中直接复制 SQLite 主文件。
2. 确认实例实际使用的完整配置目录。显式 `--config-dir` 或 `WORKBUDDY_PROXY_CONFIG_DIR` 会改变位置，不按默认位置猜测。默认 Windows 为 `%APPDATA%\WorkBuddyProxy`，macOS 为 `~/Library/Application Support/WorkBuddyProxy`。
3. 将整个配置目录复制到另一个未占用的私有备份位置，记录旧版本、CPU 架构、包 SHA256、实际配置路径和时间。完整保留 `config.json`、`usage.sqlite3` 及存在的 WAL/SHM、外观、草稿和其他文件；不要只导出供应商或只复制数据库。保留私有访问权限，备份不要上传到 Issue、日志或共享盘。
4. 独立保留匹配 CPU 架构的原版安装包及校验值。Mac 同时保留原 `.app`；Windows 保留原 setup。与可信的原发布清单核对，不仅比较版本字符串。
5. 若包含 WorkBuddy 模型同步，另行备份当前用户的 `.workbuddy/models.json`。该文件在代理配置目录外，不自动覆盖到现用 WorkBuddy 会话。以下测试没有覆盖 WorkBuddy 登录态或模型同步回退。

## 冷回滚步骤

1. 停止客户端请求，完整退出候选宿主、Core 和设置窗口，检查实际安装与配置路径。有残留进程、活跃挂载、归属不明或备份不完整时停止回滚。
2. 将当前完整配置目录重命名为一个新的私有保留目录，不删除、不覆盖。候选运行后新增的记录和设置仍保留在这里，恢复旧备份后的旧版看不到这段历史。不要手改 `PRAGMA user_version`，不要拼接数据库或 WAL，也不要让旧版尝试降级候选数据库。
3. 恢复原版程序：macOS 将当前 `.app` 移到新的保留位置，再把已校验的原 `.app` 放回同一安装路径。Windows 使用候选自己的卸载程序卸载应用文件，再用已校验的旧 setup 安装到原路径；不清理外部配置。核对版本、架构、包内资源、Windows 注册项及快捷方式，保持开机启动关闭。
4. 将升级前的完整私有配置备份复制回原配置路径，目标必须不存在。保留备份原件，不对现存目录覆盖合并。核对完整内容、配置/Key 字节与数据库；确认没有活跃的 instance/GPUI 进程记录。备份损坏则停止，不创建空配置冒充恢复。
5. 手动启动旧版，确认配置路径、供应商/模型、非默认设置和旧用量/成本，测试一次受控请求能继续记录，再恢复客户端流量。自动化使用 loopback 合成请求，真实用户需按自己的账号与费用约束检查。
6. 保留升级前备份、候选程序/配置目录和原包。候选新增记录与恢复后的旧版记录分属不同数据库，此流程不自动合并或补账。重试候选应使用单独副本，避免覆盖唯一的数据副本。

## 包来源与校验

六个旧包的完整 URL、大小与 SHA256 固定在 [`release_upgrade_sources.json`](../../scripts/release_upgrade_sources.json) 的 `official_releases` 中，分别来自公开 [v0.5.6](https://github.com/gloryhui/workbuddy-proxy/releases/tag/v0.5.6) 和 [v0.5.7](https://github.com/gloryhui/workbuddy-proxy/releases/tag/v0.5.7)。下载后以 `shasum -a 256 "包路径"`（macOS）或 `Get-FileHash -Algorithm SHA256 -LiteralPath '包路径'`（PowerShell）核对清单。校验失败即停止。

候选使用同一清单绑定 runtime `9570451` / CI `41246381635` 的三份 artifact。测试分别核对下载来源、大小、SHA 和实际安装文件。CI artifact 有保留期限；正式 RC 仍需固定可获取的原包、候选包和校验清单，不能把临时 artifact 当长期公开分发。

## 自动化验收与范围

从仓库根目录，在一次性 GitHub Actions runner 上运行：

```text
python scripts/installed_upgrade_test.py --platform PLATFORM --directory NEW_FIXTURE --output-dir REPORTS --verify-rollback
```

`PLATFORM` 为 `windows-x64`、`macos-arm64` 或 `macos-x64`。`NEW_FIXTURE` 必须不存在，报告必须位于其外。已有 Desktop workflow 的手动 `upgrade_only` 会调用三平台矩阵，下载冻结 artifact 只需要 Actions read 权限。脚本会操作 Windows 当前用户安装项，禁止在用户的普通 Windows 安装环境运行。

本机 macOS 补验必须显式加 `--local-headless-only`：仍实际替换/恢复 app，跳过原生 GUI。临时路径由系统创建，按本轮所有权记录清理，不按名称前缀批量删除。

每例用真实旧 Core 写 12 条有价记录与非默认设置，完整备份，实际升级并新写第 13 条。回滚恢复原程序及完整目录：启动前文件树字节一致、schema 为原值，旧版能再写一条有价记录，原 12 条保持；候选第 13 条只在保留目录中，保留目录字节不变。报告不包含配置、Key 或数据库。

本机 SQLite 的冷只读 WAL 打开诊断见 [原始记录](benchmarks/cold-rollback/readonly-wal-diagnostic.json)。仅已停止且 WAL 无未检查点内容时使用 immutable 读取，运行中保持普通只读，不跳过有内容的 WAL；其条件依据 [SQLite 官方说明](https://sqlite.org/wal.html#read_only_databases)。

旧版 GUI、物理键鼠、热升级、默认路径所有历史变体、WorkBuddy 会话、真实系统登录、数据库历史自动合并不在本项证明中。正式版本、公开截图及最终发布结论仍由 R04.4 / R04.5 单独验收。
