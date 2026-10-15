# 第六轮证据

对象：独立统计worktree，HEAD=e1725c3加未提交修改，schema3/API2/control6。[第六轮报告与任务](../REVIEW-ROUND6-2026-10-15.md)。

- [迁移和启动恢复](recovery-results.json)：正式0001+0002构造v2后升级失败；1200旧终态pending只修256，944残留且ready。样例启用FK，未访问用户DB。
- [缺失Start](missing-start-results.json)：写锁覆盖Start/Finish两批失败，解除后无记录、无backlog但仍degraded。
- [单条孤立费用](orphan-results.json)：原触发器故障路径已有改善，解除后可自行恢复。
- [价格边界](price-edge-results.json)：无ID带revision已拒，canonical删除成功。
- [Writer结果](store-results.json)：锁预算及清理改善。
- [39项集成](statistics-test.log)；[GPUI复跑](gpui-rechecks.json)及gpui-recheck-0/1/2.log记录聚焦失败、全量失败、最后一次全量成功。
- [源码SHA](source-sha256.json)：审查版本；后续改源码旧结果不代表新实现。

本轮117核心、核心release/fmt/clippy通过；GPUI首跑另有36/37通过一项失败（终端结果未单独保存，随后复跑原始日志已存）。其他验证边界见报告。

从主工作区复现（会覆盖对应结果，复跑前保留旧证据）：

```sh
python3 docs/statistics/review-round6-2026-10-15/recovery_probe.py
python3 docs/statistics/review-round6-2026-10-15/missing_start_probe.py
python3 docs/statistics/review-round6-2026-10-15/orphan_probe.py
python3 docs/statistics/review-round6-2026-10-15/price_edges.py
cargo run --quiet --locked --manifest-path docs/statistics/review-round6-2026-10-15/store-probe/Cargo.toml
```

helper引用开发worktree的statistics_test辅助函数；默认EXE为主工作区target/release/workbuddy-proxy，可用WBP_REVIEW_WORKTREE/WBP_REVIEW_EXE覆盖。先构建对应核心。所有进程、配置、Key、Mock、触发器和SIGKILL均为脚本自己的临时隔离实例。

Writer探针带Cargo.lock并直接引用实际源码，journal/config为最小桩。诊断exit0不代表产品正确；实际结果区分已改善与仍失败。迁移失败结果保存了核心回传的完整SQL，仅为假数据样例，用于证明错误信息边界缺陷。
