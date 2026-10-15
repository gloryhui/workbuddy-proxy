# 协议与统计联合验收

日期：2026-10-16。开发分支 `codex/upstream-protocols`；统计/分页基线 `40ec917`，请求与状态机阶段 `411516e` / `43d5a5c`，最终集成代码 `37f7738`。验收附件随后单独提交，代码验收提交不变。合并目标为本地 `main`，采用 `git merge --ff-only codex/upstream-protocols`，最终以 Git 祖先关系和两个工作区干净状态核验。

版本仍 0.5.3；管理协议 7；统计 API 2 / schema 4。没有推送、发布、安装、变更真实用户配置或用真实 Key 请求供应商。旧统计与设计 worktree 均保留。

## 最终结果

- 175 核心单测、61 GPUI 单测通过；两个 Cargo 工程严格 clippy、fmt、Release 构建通过。
- 同一组 Release 核心和 GPUI helper 在临时 app 目录中完成 15 个独立脚本，全部退出 0；[逐项结果](validation-2026-10-16/final-regression.json)。
- 原生三协议均经过独立 REST fixtures → 真实核心 → 标准 Chat 消费者 → 真实 SQLite 的 JSON/SSE、图片、普通 assistant 历史、两轮同名并行工具闭环；检查工具结果 ID、原生 opaque 状态、末尾用量、坏响应/截断/错误事件、探测与取消。
- 三协议分别验证请求进行中改价格后仍按旧价结算；删除价格后下一请求记 default_zero。100 输入 / 20 输出，在 2 / 8 元每百万 Token 的冻结单价下精确为 360000 nano。取消时未知输出/总量/费用保留 NULL。数据库不含测试提示、图片、签名或 Key。
- 模型发现覆盖原生鉴权、完整端点及网关前缀、游标转义、多页、Gemini 生成方法筛选、未保存草稿、协议/凭据能力隔离、循环/缺失游标、16 页/4096 模型/1 MiB 上限。
- 39 项统计集成检查通过；20/50/100 页大小真实前后遍历明细、分组及 321 条定价；固定窗口不会无限累计。采集模式只加载当前供应商页；原生详情显示协议。
- 原生资源脚本覆盖 1 MiB 帧、16 MiB 聚合、8 MiB 请求、4 在途/第5个429、慢读背压、取消与许可复用；采样核心 RSS 从 10.0 MiB 到 44.5 MiB，增长 34.5 MiB。这是该测试负载的采样值，不是所有输入下的内存上限或全进程 GPU 统计。

## 性能问题与修复依据

初次十万行混合价格测试出现约 1.3–1.4 秒的单次总览慢样本；复测仍存在，未写成通过。关闭外部目录刷新后通过，结合代码确认目录刷新在唯一 proxy I/O 线程同步索引/序列化/写盘。修正为阻塞池更新与持久化，JSON/磁盘 I/O 不占热缓存锁；新增确定性锁竞争回归。保留 [修复前失败结果](validation-2026-10-16/benchmark-before-fix-isolated.txt)、[离线诊断](validation-2026-10-16/benchmark-offline-diagnostic.txt)。

修复后保持外部目录刷新开启，用 100000 行 mixed 数据、7 次查询采样、100 次 relay 采样通过原预算，未放宽阈值：

| 项目 | P95 |
| --- | ---: |
| 总览 + 趋势 | 237.51 ms |
| 明细首页 | 16.22 ms |
| 游标页（含首个100条页，后续页中位约1.12ms） | 16.27 ms |
| 单条详情 | 0.49 ms |
| 分组统计 | 211.69 ms |

核心 RSS 61.11 MiB；开启采集的 relay P95 差值 -0.17 ms，属于测量波动，不宣称采集能加速。完整样本见 [benchmark-fixed.json](validation-2026-10-16/benchmark-fixed.json)。百万行全范围总览仍可能受查询预算拒绝，之前的受控超时证据保留在统计验收目录；没有把超时当快速成功聚合。

## 原生窗口与打包条件

隔离配置实际检查了协议选择、字段联动、预算校验失败保留输入、原生模型发现/勾选/添加/保存、落盘协议/profile；检查 1180×780 与 1100×700。小窗口最初挤掉保存按钮，已将表单设为可滚动区域，保存/测试固定可见。

[1100×700 窗口截图](validation-2026-10-16/ui-1100x700.png) · [四协议选择截图](validation-2026-10-16/ui-protocols.png)

初次用没有同级/helper 的裸核心运行宿主启动项脚本失败，原因是缺少该脚本要求的真实打包结构。补齐临时 app 的 Release 核心 + 匹配 GPUI helper 后，原脚本及 core_compatibility、gpui_package_test 全通过，未删除或放宽断言。临时 app 生命周期还验证旧核心替换、重复启动、关闭窗口释放 GPUI、托盘重开、未保存草稿恢复和启动项注册失败回滚。[生命周期数据](validation-2026-10-16/final-gpui-lifecycle.json)

## 复现与边界

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release --locked
cargo fmt --manifest-path gpui/Cargo.toml --check
cargo clippy --manifest-path gpui/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path gpui/Cargo.toml
cargo build --manifest-path gpui/Cargo.toml --release --locked
python scripts/native_protocol_test.py --exe target/release/workbuddy-proxy
python scripts/native_discovery_test.py --exe target/release/workbuddy-proxy
python scripts/native_resource_test.py --exe target/release/workbuddy-proxy
python scripts/statistics_test.py --exe target/release/workbuddy-proxy
python scripts/statistics_pagination_test.py --exe target/release/workbuddy-proxy
python scripts/statistics_bench.py --exe target/release/workbuddy-proxy --rows 100000 --scenario mixed --repeats 7 --relay-samples 100
```

宿主/启动项/GUI 生命周期脚本的 `--exe` 必须指向临时 app/打包目录的核心，旁边具有对应 GPUI helper，不能用裸核心冒充打包验收。CI 已接入新协议、资源、统计与分页脚本；未推送，所以没有远端 CI 结果。

[acceptance.json](acceptance.json) 把本机实现验收与 L6 实际供应商/真实 WorkBuddy 验收分开。未使用真实账号、未进行真实工具执行器或供应商账单验收，也没有 Windows 实机 GUI 结果。实际账号验收步骤和当前明确拒绝的参数见 [USAGE.md](USAGE.md)。这不影响已实现代码的本地合并；不得把本地 fixtures 结果写成生产验证。
