# 第四轮证据

对象：独立统计worktree `codex/usage-statistics`，HEAD=e1725c3加未提交实现；[第四轮报告与任务](../REVIEW-ROUND4-2026-10-15.md)。

- [API结果](api-results.json)：缺价汇总、价格过滤、假成功、canonical ID误用、0缓存价、空白价、缺Key、clear范围、有价请求SIGKILL恢复及旧v2终态pending修复缺失。
- [分页结果](paging-results.json)：as_of_id=3，第二页请求1条/金额和结算数却3条。
- [孤立恢复结果](orphan-results.json)：临时触发器拒绝cost更新，状态ready但in_flight/pending；移除故障后无新请求仍不补偿。
- [Writer结果](store-results.json)：清理/retention多批修复有效；1ms锁等待预算实际274ms。
- [统计集成日志](statistics-test.log)：37项通过；[核心兼容日志](core-compatibility.log)：Python3.13通过。
- [源码SHA-256](source-sha256.json)：审查版本身份；后续源码变化后旧结果不代表新实现。

全部测试使用临时目录、本地Mock/假Key，没有真实供应商或真实用户配置/DB。`old_v2_terminal_pending_after_restart`是按第三轮实际出现的坏组合构造的旧数据样例；不声称读取了真实用户库。

从主工作区复现Python专项（会覆盖本目录对应JSON/输出前请保留旧证据）：

```sh
python3 docs/statistics/review-round4-2026-10-15/api_probe.py
python3 docs/statistics/review-round4-2026-10-15/paging_probe.py
python3 docs/statistics/review-round4-2026-10-15/orphan_probe.py
```

helper依赖开发worktree的statistics_test辅助函数。默认核心路径为主工作区`target/release/workbuddy-proxy`，先构建对应版本；`WBP_REVIEW_WORKTREE`/`WBP_REVIEW_EXE`可覆盖。故障触发器和SIGKILL仅针对脚本自己的临时实例。

直接Writer探针带Cargo.lock，`#[path]`引用实际开发worktree源码，journal/config为最小桩：

```sh
cargo run --quiet --locked --manifest-path docs/statistics/review-round4-2026-10-15/store-probe/Cargo.toml
```

诊断脚本输出是事实观察，不把exit0当产品验收通过。常规113核心/37GPUI测试、两端fmt/clippy、核心release也已重新执行，完整终端输出未全部保存。本轮没有百万行、MSRV、Windows、真实计费和人工像素布局新证据。
