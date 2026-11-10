# 软件更新与恢复

本功能从 v0.6.1 开始提供。旧 v0.5.7 / v0.6.0 不包含此更新器；首次启用需要手动安装 v0.6.1 或后续版本。在线安装仍需下载校验后再次确认，保留旧程序和数据备份。

开发者可按 [更新与恢复源码导读](source-guide/UPDATE_RECOVERY.md) 跟踪普通 GPUI、Core、独立 helper 和只读进度窗口，理解状态、配置锁、维护标记、健康核对与失败终态。

## 界面与操作

点击工具栏“检查更新”，打开 620 × 560 的原生 GPUI 弹窗。显示最新正式 Release 的名称、版本、发布日期和完整文本说明，长内容在中间区域滚动，底部按钮固定。使用现有主题、系统字体与键盘焦点，不增加字体文件或通用图片解码器。

- **立即更新**：下载当前系统和架构的官方安装包，显示进度；代理继续运行，可以取消。
- **前往 GitHub**：打开该正式版的公开 Release 页面，由用户自行下载。
- **暂不更新**：仅在有更新或其他需要取消的状态显示；已是最新版时只保留“前往 GitHub / 完成”。关闭弹窗。关闭正在下载的弹窗不会取消后台下载；取消需要点击明确的取消操作。
- **重启并安装**：下载、校验和准备全部成功后再次确认。等待已有请求完成，期间可选择稍后安装；进入替换阶段后不可取消。

检查中、没有新版、网络失败、下载、校验、准备、待安装、等待请求、安装、完成、取消、安装失败和恢复失败均有独立反馈。检查动作写入活动日志，详细过程保留在本机状态文件；不会把管理 Key、上游配置或带令牌的重定向地址写入更新日志。

## 下载和安装边界

仅查询 `gloryhui/workbuddy-proxy` 的 GitHub 正式 Release，版本按三段数字比较。公开 HTTPS 下载源和重定向受白名单限制，安装包最大 512 MiB，元数据和日志均有限额。资产按 macOS ARM64 / Intel、Windows x64 精确匹配；校验 `SHA256SUMS.txt` 的文件名、长度和 SHA-256，不执行任意 URL 或外部传入的安装命令。

macOS 从 DMG 只读挂载、校验应用标识、主程序和设置程序版本及签名，在现有应用旁边准备同文件系统暂存。Windows 使用已校验的官方 Inno 安装器，保持当前用户安装，不自动提权。只读安装位置、缺失包、无权限、路径中存在不支持的符号链接等情况显示原因，并提供手动下载入口。签名检查不等同于 Apple 公证或 Windows SmartScreen 验收。

确认安装后暂停新的代理请求，等待现有请求（包括流式响应）结束。独立 helper 等待 Core、宿主和 GPUI 的配置锁释放，备份程序和配置，复制 SQLite 主文件及 WAL，检查备份数据库，再替换程序。新 Core 启动后保持维护状态；版本、服务和统计库健康检查通过才恢复接入请求。

失败会恢复旧程序和备份配置，然后启动旧版并实际核对健康状态。仅核对成功才报告“已恢复”；任何恢复失败或中断无法确认都报告需要人工恢复。进度由独立只读 GPUI 窗口显示，其关闭不终止 helper。不会停止、重启或改写 WorkBuddy。

## 状态、日志与备份

所有路径以本次实例实际的配置目录为准，不能根据默认目录猜测：

| 位置 | 用途 |
| --- | --- |
| `update-install-result.json` | 有界状态、时间日志、错误及备份位置 |
| `updates/<48 位随机十六进制标识>/` | 本次校验包、独立 helper、计划与诊断文件 |
| 同上 `profile-backup/` | 安装前配置和 SQLite 冷备份 |
| 应用旁 `.workbuddy-update-<标识>/previous` | macOS 安装成功后保留的旧应用；回滚时移回原位 |
| `update-helper.lock` | 防止并发安装 |
| `update-maintenance` | 安装期接入保护，内容绑定本次更新标识，不进入数据备份 |

备份不随成功自动删除。完整配置备份包含私密信息，应留在本机私有目录，不上传 Issue。中断的下载可重新检查并下载；中断的安装应按下列恢复流程核对，不能用“重新下载”掩盖未完成的恢复。

## 人工恢复

先看弹窗错误、备份位置与本机 `update-install-result.json` 中的日志。停止客户端请求，确认没有活跃 helper，完整退出宿主、Core 和设置界面；保留当前程序、当前配置与所有备份，按 [冷回滚指引](studio_v2/UPGRADE_ROLLBACK.md) 恢复成套程序和配置。不要只恢复 SQLite 主文件，不手改 schema，不合并不同版本的 WAL。

如果维护标记仍在，新 Core 会保持暂停接入并显示恢复待核对。先确认程序版本、实际配置目录、数据库与恢复结果，以及 helper 已退出；然后仅移除该配置目录下经过核实的 `update-maintenance` 单个文件，重新启动并执行受控请求。不要批量清理 `updates/`，不要删除唯一备份，也不要在无法确认恢复结果时直接移除标记。

## 接口与开发验收

本地控制协议升级为 **13**；以下命令均要求已有的管理认证：`check_update`、`update_status`、`update_start`、`update_apply`、`update_cancel`、`update_ack`。`update_status` 不加载配置快照或模型目录。生产没有测试下载源或伪造版本开关。

```text
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --manifest-path gpui/Cargo.toml
cargo clippy --locked --manifest-path gpui/Cargo.toml --all-targets -- -D warnings
python3 scripts/online_update_test.py --help
python3 scripts/online_update_macos_test.py --help
```

复用当前兼容的 Cargo 缓存。集成测试只使用本轮创建的一次性目录，启用真实用户配置和已安装应用的内核访问隔离，不覆盖 HOME。测试结束后停止所有夹具进程再清理，保留报告。macOS helper 夹具直接准备签名暂存应用，验证真实替换、重启、健康检查与失败回滚；它不代表互联网安装包下载到 DMG 准备的全链路验收。

首次实现的历史验证范围和体积记录见 [实现验收报告](https://github.com/gloryhui/workbuddy-proxy/blob/v0.6.1/docs/studio_v2/ONLINE_UPDATE_IMPLEMENTATION.md)，界面验收见 [原始报告](https://github.com/gloryhui/workbuddy-proxy/blob/v0.6.1/docs/studio_v2/online-update-gui-review.json)。Windows 在线安装 / 自动回滚实机、Intel Mac 完整 GUI 专项、真实公网新版本全链路更新、辅助技术朗读与系统安全提示仍未验证；不能将本机 macOS ARM 结果扩展为这些项目通过。
