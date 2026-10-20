# 第三轮专项证据

对象：`/Volumes/1TB/Downloads/git/workbuddy-proxy-usage-statistics`，`codex/usage-statistics`，HEAD=e1725c3加未提交修改。完整结论见[第三轮复核与施工要求](../REVIEW-ROUND3-2026-10-15.md)。

- [API原始结果](api-results.json)、[逐项输出](api-output.txt)：真实新构建核心，本地Mock、假Key、临时配置/SQLite。覆盖缺价汇总、列表参数、0缓存价、快照时间、重复新建、清理越界、未发上游、正常停止、SIGKILL/重启和恢复元数据写失败。
- [Writer维护结果](store-results.json)：直接引用实际store/mod源码，1200行、500批次，观察clear/retention提前完成。
- [源码SHA-256](source-sha256.json)：审查代码身份；后续修改后旧结果不再代表新版本。
- [回归脚本结果](regression-results.json)：其中core_compatibility首次因系统Python无tomllib未运行成功；[改Python3.13后的输出](core_compatibility.python313.log)确认通过。这是环境修正，不是代码修复。

Python复现（从主工作区执行）：

```sh
python3 docs/statistics/review-round3-2026-10-15/api_probe.py
```

脚本使用同目录helper及开发worktree中的statistics_test辅助函数，默认EXE为脚本所在仓库的`target/release/workbuddy-proxy`；`WBP_REVIEW_WORKTREE`/`WBP_REVIEW_EXE`可覆盖。先构建审查版本核心。会覆盖同目录API JSON，所以保留历史证据后再复跑。故障注入只针对临时测试库，脚本正常清理进程/临时目录，不接触用户数据库。

Writer探针：

```sh
cargo run --quiet --locked --manifest-path docs/statistics/review-round3-2026-10-15/store-probe/Cargo.toml
```

其`#[path]`相对引用本仓库源码，journal/config为最小桩，Writer/SQL/维护逻辑均用实际产品源码。输出是缺陷观测，退出0不是产品行为通过；修复后应在正式测试中断言预期删除范围/条数，而非保留缺陷结果。

本轮重新执行110核心/37GPUI/37统计集成、两端fmt/clippy、核心release和五类兼容回归。完整常规测试终端输出未全部保存，本目录保留专项原始结果及五类回归日志。GPU像素布局、Rust1.88、Windows、真实供应商、三类百万行性能未在本轮重新验证。

路径约定：helper 默认从自身文件定位仓库，不依赖当前目录；支持 `WBP_REVIEW_WORKTREE`、`WBP_REVIEW_EXE` 和本机 `CARGO_TARGET_DIR` 覆盖，Windows 默认二进制自动加 `.exe`，命令中的 `python3` 改为 `python`。Rust 探针相对引用所在仓库源码，审查其他 worktree 时应使用该 worktree 内的副本；探针依赖历史接口与缺陷行为，复跑前核对版本，旧结果不代表当前版本通过。
