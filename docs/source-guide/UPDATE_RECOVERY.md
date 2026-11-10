# 软件更新与恢复：从版本提示到独立 helper

本篇面向 Java / Spring Boot 开发者，解释更新为什么分为检查、下载准备、确认安装和恢复四段，以及原进程退出后谁继续处理。操作方式与人工恢复步骤见 [软件更新与恢复](../AUTO-UPDATE.md)；这里按当前源码说明调用关系，不执行真实安装。

## 1. 先区分控制入口与进程角色

```text
GPUI update_notice
    ↓ backend Job → 本地鉴权控制命令
Core proxy::action → Bridge::command_inner
    ├─ check_update → update::check：查询版本与本机安装能力
    ├─ update_start → Updater::start：下载、校验、暂存 → Ready
    ├─ update_apply → Updater::apply：暂停新请求、排空旧请求
    │                                      ↓
    │                           独立 update-helper 进程
    │                           等锁 → 冷备份 → 替换 → 启动 → 核对
    │                                      ↓
    │                           update-install-result.json
    └─ update_status / update_cancel / update_ack
                                           ↓
                              独立 update-ui 只读进度窗口
```

入口分别是 [`update.rs`](../../src/update.rs)、[`updater/mod.rs`](../../src/updater/mod.rs)、[`updater/install.rs`](../../src/updater/install.rs)、[`update_notice.rs`](../../gpui/src/ui/update_notice.rs) 和 [`update_monitor.rs`](../../gpui/src/update_monitor.rs)。控制命令分派在 [`proxy.rs`](../../src/proxy.rs)。

类比 Java：检查接口类似查询服务，下载与准备是后台任务，安装 helper 是能独立于宿主存活的外部进程。它们没有共享一个 `@Transactional` 边界。文件替换、进程退出、数据库备份、HTTP 健康核对靠显式步骤和恢复分支协调。

普通设置界面负责发命令和显示响应，Core 负责下载协调，helper 负责原进程退出后的安装。独立进度界面又是另一个只读角色，不是 helper 的控制器。上述动作只针对本产品，不停止或重启 WorkBuddy。

## 2. 检查版本不等于开始下载

`update::check` 读取固定公开 Release 源，版本只接受可选 `v` 前缀和三段无符号数字；按 `[major, minor, patch]` 比较，不接受预发布后缀，不按字符串排序。

成功检查结果可以保存在 `update-cache.json`。缓存受大小、24 小时有效期与合法版本限制；`force=true` 绕过缓存重新查询。网络错误不会被伪装成一次新的成功缓存，写缓存失败也不会把已经取得的查询结果变成失败。

响应中要分清：

| 字段 | 表达什么 |
| --- | --- |
| `available` | 选中的正式版本是否高于当前版本 |
| `online_supported` | 当前安装位置、平台和 Release 资产是否支持自动安装 |
| `online_error` | 自动安装能力检查失败的原因 |
| `cached` | 这次版本信息是否来自有效缓存 |

因此“有新版本”不保证本机能自动安装。真正 `update_start` 时会重新检查版本、维护标记、活跃 helper、安装目标和资产名称，再按选定 tag 重新获取元数据。旧缓存没有把任意 URL 授权成安装包。

## 3. 下载线程与 Rust 共享状态

`Updater` 使用 `Arc<Self>` 让下载/安装协调线程与 Core 共享实例，`Mutex<Inner>` 保护 Progress 与 prepared Plan，`AtomicBool` 传递取消信号。

Java 可类比共享服务对象、短时 synchronized 区块和可见性受控的取消标志。`Arc::clone` 克隆引用计数句柄，不复制安装包；锁保护内存字段，不意味着磁盘文件自动提交。

`start` 创建 `release-download` 标准线程，在线程内创建 current-thread Tokio runtime。网络 await 在这条线程自己的 runtime 执行；磁盘暂存和平台命令也在下载准备链路，不把整个安装包下载塞进代理请求的 I/O 循环。

Progress 会写结果文件，但部分高频下载字节只更新内存。不能假定磁盘文件与实时 `update_status` 每一刻都相等。`prepared` 只在当前进程内持有；反序列化出 Ready 状态并不能重新获得一个可安装 Plan。

## 4. 从元数据到 Ready：校验哪些东西

| 边界 | 当前代码检查 |
| --- | --- |
| 平台资产 | macOS ARM64 / x64 DMG、Windows x64 安装器；其他平台拒绝自动安装 |
| 正式版本 | 精确 tag、非 draft、非 prerelease |
| 安装包元数据 | 唯一精确文件名、固定仓库 Release URL、非零且不超过 512 MiB |
| 下载客户端 | 公开 HTTPS、443 端口、限定 GitHub 主机重定向及跳转次数 |
| 校验文件 | 唯一 `SHA256SUMS.txt` 资产，目标文件只有一条合法摘要：64 个十六进制字符，即 32 字节 |
| 写包 | `create_new` 避免覆盖已有文件；边下载边限制长度、计算 SHA-256 |
| 下载结束 | 同步文件，核对实际字节数和摘要，再进入 prepare |

元数据与校验文件自身也限制响应大小。下载失败时只在本次成功创建包文件后尝试移除它，不把同名已有文件当成自己的残留。

SHA-256 用于检查包与所选 Release 摘要一致；摘要来自同一发布源，不能把它夸大为独立第三方签名认证。macOS 暂存应用还会检查 bundle 标识、Core/设置程序结构、版本与 codesign；代码签名校验也不等于完成公证或系统安全提示验收。

`install::prepare` 在安装目标旁建立同一文件系统的暂存目录，提前发现不可写位置。Mac 只读挂载 DMG、复制并再次验证暂存应用；Windows 准备安装器并复制旧程序备份。随后复制独立 helper 与进度 UI、保存 plan.json。成功才进入 Ready，等待用户明确点击“重启并安装”。

## 5. 状态决定能否取消，busy 不是一个安装状态

状态 DTO 在 [`dto.rs`](../../src/updater/dto.rs)。下载准备通常按下面推进：

```text
Downloading → Verifying → Preparing → Ready
Ready → Waiting → Installing → Complete
                            ↘ Failed / RecoveryFailed
```

这是主路径，失败、取消和重启恢复还有单独分支：

- Downloading / Verifying / Preparing：取消标志由后台任务检查，准备完才观察到取消时还要丢弃暂存计划。
- Ready：取消会取走 prepared plan，调用 discard 清暂存；成功后记录 Cancelled，discard 失败则返回错误。它不会清除所有历史更新目录和诊断。
- Waiting：取消等待，回到 Ready 并重新允许新请求，保留已验证计划，便于稍后再安装。
- Installing：普通取消命令拒绝；此时需要 helper 的恢复机制。

Ready 也属于 `busy()`，用于阻止并发 start 或 acknowledge。它不意味着一直消耗 CPU，也不能当成“所有 busy 状态都可取消”。命令成功返回的是当前快照，后台动作可能仍在继续。

## 6. 安装确认：gate 与全部 permit 各管一件事

`Updater::apply` 要求 phase=Ready、tag 匹配且当前进程持有 Plan，然后置 `update_installing` gate 并进入 Waiting。gate 让共享的 `relay_to` 转发入口拒绝新请求，当前两条 Chat completions 路由都走它；健康和鉴权管理接口仍可用。

`release-apply` 线程等待获得代理信号量的全部 `MAX_CONCURRENT` 许可。已有请求持有各自许可，流式许可随响应体生命周期释放；拿到全部许可才说明这些受控请求已排空，不是只等到响应头返回。当前循环约每 100ms 检查取消，不设置一个到了就强杀请求的固定等待截止时间。

gate 和 semaphore 不能互相替代：前者挡新请求，后者等待现有许可归还。取得全部许可后进入 Installing，启动 helper，等待其就绪，再退出当前应用；线程创建或交接失败会走错误处理，不能把“开始等待”当成“已经替换”。

## 7. helper 就绪、三把 profile 锁与维护标记

`launch_helper` 执行本次 session 中复制出的 Core，以 `--apply-update plan.json` 启动，等待 helper-ready 标记或进程失败。原应用不会只凭 spawn 成功就退出。

helper 自己重新读取有界 Plan，核对规范化路径、48 位 session token、平台、目标、暂存和备份位置；取得 `update-helper.lock` 防并发 helper，再核对包大小/SHA 和暂存应用。Plan 是序列化输入，不能因为它由自己生成过就跳过二次验证。

就绪后 helper 等待取得 `instance.lock / frontend.lock / gpui.lock` 三把锁，正常安装等待最多 60 秒，并检查 abort 标记。这三把锁证明占用同一配置的应用角色已释放；helper 锁只证明没有另一个安装 helper，两者职责不同。

持有三把锁后才用 `create_new` 认领 `update-maintenance`，把 session token 写入并同步。这不是普通互斥锁：新 Core 启动时会看到持久化维护标记，健康核对前暂停普通聊天转发；健康与鉴权管理接口仍可用。helper 清理标记时还要核对 token，不能删掉其他更新的保护。

## 8. 冷备份、替换与健康核对

helper 在三把 profile 锁仍持有时运行 `backup_profile`。备份跳过运行态锁、实例描述、更新 session、维护/结果等文件，拒绝遇到的符号链接；其余配置与数据包含 SQLite 主文件和 WAL。备份中存在 usage.sqlite3 时还做只读 quick_check。

这相当于应用停止写入后的冷备份，不能只复制主数据库后继续使用新版产生的旧 WAL。备份可能包含私密配置，路径只用于本机恢复。成功后仍保留程序和数据备份。

Mac 的 replace 先把旧应用移动到 previous，再把 new.app 移到安装位置；第二步失败有就地恢复尝试。Windows 执行已校验安装器，传固定参数及目标目录。返回成功仍不等于新服务可用。

替换后 helper 释放三把锁，再启动新应用。`healthy` 从实际配置目录的 instance.json 找本机端口，依次核对：

1. `/health` 的 app、ready 和精确版本。
2. 用管理认证调用 `stats_health`，确认统计状态 ready。
3. 通过后才清自己持有的维护标记，并记录 Complete。

健康检查当前最多等待约 45 秒；它验证上述服务与统计就绪，不代表真实供应商聊天、所有 UI 或跨平台安装均已验收。

## 9. 失败后怎样恢复，为什么 Ok 不代表更新成功

`run_helper` 跟踪是否已取得应用退出锁、是否进入可能改写程序的阶段、是否拥有维护标记。Windows 安装器执行前便保守地标记可能替换；Mac 替换失败时也会检查旧备份是否已形成，决定是否需要恢复。

需要恢复时先尝试停止当前应用、重新取得 profile 锁，再 `rollback → restore_profile`。Mac 在当前目标仍存在时将它移到 `stage_root/failed`，然后把 `stage_root/previous` 中的旧应用移回安装位置；Windows 从旧程序备份复制。配置恢复会处理备份中不存在的新 SQLite sidecar，并复制备份内容，不能把它描述成对整个配置目录的精确镜像删除。

恢复未报错且应用已退出过时，还要启动旧版并核对其版本和服务。确认结果后再处理本次维护标记：

| 终态 | 可得出的结论 |
| --- | --- |
| Complete | 新版版本、服务与统计就绪检查通过，维护标记已按归属清除 |
| Failed | 需读错误：可能尚未替换，或旧版已恢复并经核对 |
| RecoveryFailed | 恢复步骤失败或结果无法确认，按人工恢复指引处理 |

特别注意：`run_helper` 会捕获安装主流程的错误，写入 Failed / RecoveryFailed 后仍返回 `Ok(())`。早期计划校验、结果文件写入等错误仍可能返回 Err。进程退出码、函数 Result 和持久化业务终态不是同一个东西；排错必须读 Progress/error/logs。

## 10. 进度界面、重启与确认提示

普通 `update_notice` 通过后台 Job 发命令，约每 500ms 尝试轮询一次 update_status，以 polling 标志避免并发轮询。首次成功读取终态可能自动展示结果；解析或请求失败不能伪造成功状态。关闭下载弹窗不会发送 cancel，取消需要明确操作。

helper 启动的独立 `update_monitor` 约每 250ms 读结果文件，不打开 profile 和三把锁，也不接管安装。窗口关闭只结束观察进程；读失败保留旧状态，不代表 helper 结束。

Core 重启时 `Updater::new` 对中断准备与无法确认的安装分别报告 Failed 或 RecoveryFailed；结果文件不会自动重建内存 Plan。`status` 在特定无 prepared 状态下重新读取 helper 结果，`acknowledge` 也不会抹掉仍带维护标记的 RecoveryFailed 警告。关闭提示与完成恢复是不同动作。

## 11. 阅读顺序和现有测试入口

建议按 `update_notice → proxy::action → Bridge::command_inner → update::check → Updater::start → prepare → apply → run_helper → healthy/rollback → update_monitor` 设置断点。先画出正常流程，再推演“下载中取消”“等待流式请求时取消”“新版启动失败”“恢复健康核对失败”。

本轮只核对下列测试存在，未执行业务测试或真实安装：

| 文件 | 测试名 | 阅读重点 |
| --- | --- | --- |
| [`update.rs`](../../src/update.rs) | `numeric_versions_and_cache` | 数字版本与缓存 |
| [`updater/mod.rs`](../../src/updater/mod.rs) | `partial_or_corrupt_downloads_are_removed_and_existing_files_survive` | 失败包与已有文件边界 |
| 同上 | `installation_waits_for_active_requests_and_can_be_deferred_without_launching_helper` | 排空与稍后安装 |
| 同上 | `unresolved_maintenance_warning_survives_dialog_dismissal` | 关闭不能掩盖恢复问题 |
| [`install.rs`](../../src/updater/install.rs) | `held_profile_locks_prevent_replacement` | 配置锁与替换边界 |
| 同上 | `cold_profile_backup_keeps_wal_and_rollback_restores_data` | 冷备份和数据恢复 |
| 同上 | `helper_paths_are_bound_to_exact_session_and_staging_directory` | Plan 路径绑定 |
| [`update_notice.rs`](../../gpui/src/ui/update_notice.rs) | `requires_typed_result_and_safe_versions_but_tolerates_older_metadata` | 界面响应校验 |

安装属于真实副作用，修改上述行为时应按项目 L3/R 规范做隔离验证。注释审核与测试名存在性检查不证明 Windows 实机回滚、Intel Mac GUI 或公网新版本完整更新通过；已有验证范围继续以 [更新说明](../AUTO-UPDATE.md) 为准。
