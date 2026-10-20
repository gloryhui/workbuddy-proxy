# 第五轮复核证据

对象：独立统计worktree `codex/usage-statistics`，HEAD=e1725c3加未提交实现。[第五轮报告与施工要求](../REVIEW-ROUND5-2026-10-15.md)。

- [API结果](api-results.json)：上轮崩溃、旧pending、重复新建、blank价等主要失败样例已有改善。
- [分页结果](paging-results.json)：冻结水位下，第二模型现在请求1/结算1/2800000 nanos，修复有效。
- [孤立费用结果](orphan-results.json)：故障及解除故障后仍ready/in_flight/pending，U05未修。
- [价格边界](price-edge-results.json)：无ID却带revision可改已有价；canonical price_id删除仍报rule_id required。
- [Writer结果](store-results.json)：清理/保留多批正确，1ms持锁预算实际280ms。
- [39项集成日志](statistics-test.log)；[源文件SHA](source-sha256.json)。

复跑使用临时配置/数据库、假Key、本地Mock；没有真实供应商或真实用户数据库。旧v2终态pending为按第四/第三轮实际问题构造的隔离样例。启动修复有界性与GUI未改动为静态源码审查，不伪称已完成大样例性能或人工界面验收。

主工作区下的Python复现命令（会覆盖同目录对应结果，复跑前保留旧证据）：

```sh
python3 docs/statistics/review-round5-2026-10-15/api_probe.py
python3 docs/statistics/review-round5-2026-10-15/paging_probe.py
python3 docs/statistics/review-round5-2026-10-15/orphan_probe.py
python3 docs/statistics/review-round5-2026-10-15/price_edges.py
```

helper引用开发worktree的statistics_test辅助函数。默认EXE为脚本所在仓库的`target/release/workbuddy-proxy`，先构建审查版本；WBP_REVIEW_WORKTREE/WBP_REVIEW_EXE可覆盖。故障触发器与SIGKILL只作用于脚本自己的临时实例。

直接Writer探针引用实际源码，journal/config是最小桩，Cargo.lock已保存：

```sh
cargo run --quiet --locked --manifest-path docs/statistics/review-round5-2026-10-15/store-probe/Cargo.toml
```

这些脚本是诊断观测，不能以exit0替代修复验收。114核心/37GPUI测试与核心release/fmt/clippy也已重新执行，完整终端输出未全部保存；其他验证边界见报告。

路径约定：helper 默认从自身文件定位仓库，不依赖当前目录；支持 `WBP_REVIEW_WORKTREE`、`WBP_REVIEW_EXE` 和本机 `CARGO_TARGET_DIR` 覆盖，Windows 默认二进制自动加 `.exe`，命令中的 `python3` 改为 `python`。Rust 探针相对引用所在仓库源码，审查其他 worktree 时应使用该 worktree 内的副本；探针依赖历史接口与缺陷行为，复跑前核对版本，旧结果不代表当前版本通过。
