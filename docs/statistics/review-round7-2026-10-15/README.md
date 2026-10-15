# 第七轮证据

对象：独立统计worktree，HEAD=e1725c3加未提交修改，schema3/API2/control6。[第七轮报告与任务](../REVIEW-ROUND7-2026-10-15.md)。

- [1200条恢复](recovery-1200-results.json)：v2升级与原1200样例已改善。
- [20000条恢复](recovery-20000-results.json)：16640已结算、3360 pending、backlog0、ready；等候后不变。
- [内存孤立入口](missing-start-results.json)：锁住Start/Finish后解除，真实核心仍degraded，请求0、backlog0。
- [39项集成日志](statistics-test.log)；[源码SHA](source-sha256.json)。

本轮119核心、37GPUI单测、核心release/fmt/clippy通过，完整终端输出未全部保存。其他验证边界见报告。

从主工作区执行（会覆盖对应JSON，复跑前保留旧证据）：

```sh
python3 docs/statistics/review-round7-2026-10-15/recovery_probe.py
WBP_REVIEW_COUNT=20000 python3 docs/statistics/review-round7-2026-10-15/recovery_probe.py
python3 docs/statistics/review-round7-2026-10-15/missing_start_probe.py
```

helper引用开发worktree的statistics_test辅助函数。默认EXE为主工作区target/release/workbuddy-proxy，先构建对应版本，可通过WBP_REVIEW_WORKTREE/WBP_REVIEW_EXE覆盖。旧数据为正式schema和真实调用模板构造，开启FK；数据库、配置、Key、Mock、进程和故障均属于脚本自己的临时隔离实例，未接触用户配置或真实供应商。

探针是观测资料，不能把exit0当产品验收；结果分别明确已改善和仍失败，控制器/人工界面验收另行完成。
