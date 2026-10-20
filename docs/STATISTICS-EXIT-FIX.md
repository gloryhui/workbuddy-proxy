# 统计异常退出误报修复

日期：2026-10-16。故障基线：本地测试包 `964d1c0`，版本 0.5.3。

## 原因

桌面应用有核心、托盘、设置窗口三个进程。`frontend::run` 为托盘创建 `Bridge::new`，该构造函数无条件启动统计写入器。于是核心和托盘各自打开同一个统计数据库、各建一条运行记录。

核心先启动，托盘随后启动时的恢复扫描将仍存活的核心标为 `unclean`。托盘退出时也没有统计运行的正常结束记录；下一次启动又检测到托盘遗留的运行。由此造成使用正常的 ⌘Q 退出后，提示仍反复出现。

只读核对发现用户数据库中存在每次启动相隔约 40 毫秒的成对记录。没有读取聊天正文或修改用户数据库。新版生命周期回归运行在旧 Release 二进制上，确认一次启动得到 `[(结束时间, unclean), (NULL, NULL)]`，因“应只有一条核心运行”断言失败。

## 改动

- 托盘使用独立的 `Bridge::for_controller`，只转发管理命令到核心。
- `StatsHandle::for_controller` 不启动线程、不创建或打开 SQLite、不进行恢复、不产生运行记录。核心继续使用原有完整统计实例。
- 构造 Bridge 时先校验路由，再启动统计，避免无效路由导致孤立写入器。
- 总览已有异常退出质量横幅时，健康栏不再重复显示同一条提示；横幅尚未加载时保留健康栏提示。
- 不删除、重标或改价历史调用，不将真正的异常退出强行标成正常。

## 验证

- 182 项核心测试、63 项 GPUI 测试通过，两个工程严格 clippy、格式检查与 Release 构建通过。
- 新增单测验证控制器不创建任何文件、不恢复仍存活的核心、不留下运行记录；核心正常退出后再次打开没有异常退出标记。
- `scripts/gpui_package_test.py` 增加真实三进程运行断言：启动仅一条运行记录；关/开设置窗口不增加运行；退出时核心落盘 `normal`；重开无运行异常警告。
- 使用临时签名应用及隔离配置，通过 CUA 实际发送 ⌘Q 给设置窗口，验证托盘和核心都结束、数据库正常结束标记落盘，重开无误报。测试没有退出真实用户实例。
- `scripts/statistics_test.py` 验证 41 项：SIGKILL 仍将未完成请求恢复为 `interrupted/unclean_shutdown`，健康状态与总览仍返回异常警告；随后正常退出再重开，运行警告消失，原有历史异常记录保留。

复现命令：

```sh
cargo test --quiet
cargo clippy --all-targets -- -D warnings
cargo build --release --locked
cargo test --manifest-path gpui/Cargo.toml --quiet
cargo clippy --manifest-path gpui/Cargo.toml --all-targets -- -D warnings
cargo build --manifest-path gpui/Cargo.toml --release --locked
python3 scripts/statistics_test.py --exe target/release/workbuddy-proxy
# Windows 使用 python，可执行文件名加 .exe；打包目录按本机实际路径替换
python3 scripts/gpui_package_test.py --exe "dist/packaged/workbuddy-proxy" --output bench-results/lifecycle.json
```

实际 ⌘Q 检查在最后一条命令中追加 `--wait-for-cmd-q`，等待 `READY_FOR_CMD_Q` 后操作隔离的设置窗口。`--exe` 旁必须有匹配 GPUI helper 或完整 app 目录。

## 用户升级

新包不自动安装、不操作用户现有进程。替换前正常退出旧版。首次启动修复版时，仍可能检测到旧版遗留的未结束运行，因此保留一次异常提示；使用修复版正常退出一次并重开后，本次运行的异常提示应清除。历史未知用量不会因清除提示而被补齐，真实历史异常记录继续保留。

如果修复版正常退出后仍反复提示，应检查真正的退出/写盘失败，不能直接清库或屏蔽告警。
