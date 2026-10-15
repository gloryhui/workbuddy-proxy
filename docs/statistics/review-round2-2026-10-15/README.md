# 第二轮复核证据

日期：2026-10-15。对象：`/Volumes/1TB/Downloads/git/workbuddy-proxy-usage-statistics`，`codex/usage-statistics`，HEAD `e1725c3` 加未提交实现。结论和修复要求见[第二轮报告](../REVIEW-ROUND2-2026-10-15.md)，下一轮使用[合并开发Prompt](../NEXT-ITERATION-COMBINED-PROMPT.md)。

## 结果和边界

- [API结果](api-results.json)：真实核心进程、临时配置/SQLite、本地Mock。确认JSON错误、冲突usage、正常停服、健康空桶、常见锁恢复；同时确认缺价仍未知、无单价快照、引用后价格仍不可编辑。
- [汇总分页结果](group-results.json)：原样使用服务器游标被拒；改key绕过错误校验后SQL聚合错误。文件内SQL为隔离测试生成，不包含真实用户数据。
- [直接源码探针观测](probe-observations.txt)：筛选覆盖、64事件锁等待、Collector修复。
- [源码SHA-256](source-sha256.json)：五个关键被审查源码的快照身份。后续改源码后旧结果不再代表新实现。

核心105项、GPUI34项、统计集成34项、两端fmt/clippy以及核心release通过。此处计数记录本轮执行结果，没有保存完整终端日志；原始专项观察保存在上述文件中。没有重新执行百万行性能、Rust1.88、Windows或完整人工界面验收，也没有真实供应商计费证据。

## 复现专项观察

Python探针使用开发worktree的`scripts/statistics_test.py`辅助函数；已保存本地[helper.py](helper.py)，不依赖原临时目录。默认使用上述worktree和主工作区`target/release/workbuddy-proxy`，可用`WBP_REVIEW_WORKTREE`、`WBP_REVIEW_EXE`覆盖。必须先对审查版本构建release。脚本仅启动临时实例，使用假Key和本地Mock；结果会覆盖本目录对应JSON，复跑前自行保留旧证据。

```sh
python3 docs/statistics/review-round2-2026-10-15/api_probe.py
python3 docs/statistics/review-round2-2026-10-15/group_probe.py
```

三个Rust探针保存各自Cargo.toml、Cargo.lock和main.rs，通过绝对`#[path]`直接引用实际开发worktree源码，不是重写产品逻辑。路径变更时先调整`#[path]`。Writer探针只有journal/config的最小桩，直接调用真实Writer；它缩短busy_timeout至30ms以限制复核时间，2474ms是该设置下的观测，不是生产2秒配置的实测值。

```sh
cargo run --quiet --locked --manifest-path docs/statistics/review-round2-2026-10-15/scheduler-probe/Cargo.toml
cargo run --quiet --locked --manifest-path docs/statistics/review-round2-2026-10-15/store-probe/Cargo.toml
cargo run --quiet --locked --manifest-path docs/statistics/review-round2-2026-10-15/collector-probe/Cargo.toml
```

Scheduler探针断言的是“观察到当前缺陷”；退出0只表示复现匹配，不能作为修复验收。修复后需要改为正式产品回归，断言两种筛选都能派发。其他诊断探针也不能替代正式回归。

## 本轮基线命令

在开发worktree中执行，GPUI额外设置`RUST_MIN_STACK=67108864`；核心复用主工作区target缓存。测试范围的完整解释见审查报告。

```sh
export CARGO_TARGET_DIR=/Volumes/1TB/Downloads/git/workbuddy-proxy/target
cargo test --locked --quiet
cargo build --release --locked
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
RUST_MIN_STACK=67108864 cargo test --manifest-path gpui/Cargo.toml --locked --quiet
cargo fmt --manifest-path gpui/Cargo.toml --all -- --check
RUST_MIN_STACK=67108864 cargo clippy --manifest-path gpui/Cargo.toml --locked --all-targets -- -D warnings
python3 scripts/statistics_test.py --exe /Volumes/1TB/Downloads/git/workbuddy-proxy/target/release/workbuddy-proxy
```

主工作区的`python3 docs/statistics/validate_design.py`验证v2设计SQL；它不能证明实际Rust v1→v2迁移已实现。
