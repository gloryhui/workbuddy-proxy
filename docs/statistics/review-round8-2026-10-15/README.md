# 第八轮审查证据

对象源码及SHA见`source-sha256.json`，命令结果摘要见`verification-summary.json`。

- `integration-output.txt`：真实debug核心+临时配置+Mock，39项通过。
- `missing-start-results.json`：旧R7-02改善，解除锁后最终ready，无伪造请求，loss保留。
- `startup-failure-results.json`：真实核心启动时单行结算错误后ready/pending1/backlog0，故障解除后不自动修复。
- `scan-failure-output.txt`：直接引用当前Rust源模块的独立store探针，失败行扫描越过且未登记待办。
- `runtime-scan-results.json`：实际writer调度采样；20k只观察前9秒，没有声称等待全部结束。

所有配置、DB、端口、Key、Mock、故障trigger均为临时隔离样例。探针exit0表示观测脚本执行完毕，不等于产品验收通过。各脚本结果可能复写同名JSON，复跑前保留旧证据。

主仓库执行：

```sh
WBP_REVIEW_EXE=/Volumes/1TB/Downloads/git/workbuddy-proxy-usage-statistics/target/debug/workbuddy-proxy python3 docs/statistics/review-round8-2026-10-15/startup_failure_probe.py
WBP_REVIEW_EXE=/Volumes/1TB/Downloads/git/workbuddy-proxy-usage-statistics/target/debug/workbuddy-proxy python3 docs/statistics/review-round8-2026-10-15/runtime_scan_probe.py
WBP_REVIEW_EXE=/Volumes/1TB/Downloads/git/workbuddy-proxy-usage-statistics/target/debug/workbuddy-proxy python3 docs/statistics/review-round8-2026-10-15/missing_start_probe.py
CARGO_TARGET_DIR=/Volumes/1TB/Downloads/git/workbuddy-proxy-usage-statistics/target cargo run --quiet --manifest-path docs/statistics/review-round8-2026-10-15/store-probe/Cargo.toml
```

修复后应让正式回归断言正确行为；当前store探针末尾断言是为了证明旧缺陷重现，不能把这个断言保留为产品验收标准。
