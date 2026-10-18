# 命令组、隔离与证据

本文件是开发阶段应执行的命令，**设计交付未运行它们**。每阶段新增测试名称固定以 workbuddy_import 前缀标识，但不得使用过滤器“匹配到0个测试”冒充通过。脚本必须先检查源目录是测试sandbox，且在输出中给出不含用户私有路径的隔离证据。

所有命令从专用worktree根执行；不要复制main的target产物当新构建。Rust依赖使用 --locked，不更新Cargo.lock。环境固定：

```bash
export WORKBUDDY_PROXY_CATALOG_TEST_URL=http://127.0.0.1:1/catalog
export RUST_TEST_THREADS=1
```

新Python集成脚本自行创建 TemporaryDirectory，把HOME、USERPROFILE、APPDATA（Windows）及--config-dir都指向临时路径；生产worker root构造函数测试注入只在Rust测试使用。脚本结束回收子进程；任何启动app/helper子进程都传完整隔离环境，不能用会丢环境的系统open启动真实安装应用。

## G00 / baseline

```bash
git status --short --branch
git rev-parse HEAD
git worktree list
python3 docs/workbuddy-import/validate_spec.py
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo fmt --manifest-path gpui/Cargo.toml --check
cargo clippy --manifest-path gpui/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path gpui/Cargo.toml --locked
```

## G01～G07 / schema, parser, reader, classification, runtime, lifecycle, api

每阶段运行下面命令，额外执行该阶段对应测试模块，保留实际运行test数量；不允许只跑一个虚构命名的过滤器。

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

G01迁移、G02parser、G03reader、G04classification、G05runtime、G06lifecycle、G07api各自测试都必须在上述完整cargo test中可发现。阶段专项日志需标明其用例名及通过数。

G04加已有同步/端点回归，G07加统计/分页/定价集成，先构建当前核心：

```bash
cargo build --release --locked
python3 scripts/workbuddy_endpoint_check_test.py --exe target/release/workbuddy-proxy
python3 scripts/multi_routing.py --exe target/release/workbuddy-proxy
python3 scripts/statistics_test.py --exe target/release/workbuddy-proxy
python3 scripts/statistics_pagination_test.py --exe target/release/workbuddy-proxy
python3 scripts/price_catalog_test.py --exe target/release/workbuddy-proxy
python3 scripts/workbuddy_import_test.py --exe target/release/workbuddy-proxy
```

Windows实际exe后缀为.exe。新增脚本CLI固定 --exe、--output；--output缺省为忽略目录 `dist/workbuddy-import-validation`，报告写JSON并对失败返回非零；不能只print失败后exit0。

## G08 / ui

```bash
cargo fmt --manifest-path gpui/Cargo.toml --check
cargo clippy --manifest-path gpui/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path gpui/Cargo.toml --locked
cargo build --manifest-path gpui/Cargo.toml --release --locked
python3 scripts/workbuddy_import_ui_test.py --exe target/release/workbuddy-proxy --ui gpui/target/release/workbuddy-proxy-gpui --evidence dist/workbuddy-import-review/<实际原生矩阵目录>
```

UI脚本只可自动测试它真实能够驱动的事件/控制接口。没有原生事件自动化能力时，返回标记为NOT_RUN的报告，并按UI-SPEC用实际GUI工具执行人工验收；不能写截图文件就宣称点击通过。UI操作可由有电脑工具的执行者完成，DeepSeek不具备工具时必须交接证据缺口。

## G09 / release_gate

先执行G00、G07、G08，再执行当前 `.github/workflows/ci.yml` 和 `desktop.yml` 中所有适用命令；以仓库实际脚本CLI为准，先读帮助，不凭空猜flag。

```bash
python3 scripts/workbuddy_import_bench.py --exe target/release/workbuddy-proxy --output dist/workbuddy-import-validation/S10.03-strict
python3 scripts/workbuddy_import_scheduling.py --exe target/release/workbuddy-proxy --output dist/workbuddy-import-validation/scheduling
git diff --check
git diff --stat
git status --short
```

将新增import集成脚本接入核心和desktop工作流；benchmark可作为独立本地验收，避免在不稳定CI机器上虚报严格延迟门槛。core_compatibility、gpui_package_test、package脚本需要按各自帮助和现有用法运行；新协议11必须实际验证。禁止为了更快而去掉既有工作流步骤。

## 证据文件模板

每个 `evidence/Sxx.yy.md` 应包括：任务ID/ACID、输入commit、修改路径、关键不变量、测试命令与退出码、日志相对路径、至少一个针对错误场景的结果、结论、下一项。原始大型日志写dist忽略目录，证据摘要留docs（严禁任何用户私有数据）。设计阶段不创建伪造PASS报告。

## 接手后严格验收入口

`workbuddy_import_bench.py` 现为严格矩阵入口：固定100k/1M、1000文件/单巨型、各3次；每阶段保留原始样本、程序哈希和数据库诊断快照。默认逐项执行，任一失败立即停止扩展；`--aggregate-only` 只校验已有12份同程序报告，缺项返回非零。单阶段命令为：

```bash
python3 scripts/workbuddy_import_performance.py --exe target/release/workbuddy-proxy --records 1000000 --layout many --idle-seconds 300 --output dist/workbuddy-import-validation/scale-1m-many
```

矩阵结果只覆盖规模/查询/增量/资源部分，不能替代调度、原生GUI、多平台和安装验收。OS写失败/强杀恢复见 `workbuddy_import_faults.py` 的生产链路；不能用不可读源文件替代数据库写失败。

UI操作统一使用授权的原生电脑工具，保存窗口自身原生JPEG、操作后的AX及实际运行二进制哈希，再由 `workbuddy_import_ui_test.py --evidence ...` 验证。脚本无 `--evidence` 返回NOT_RUN及非零；程序哈希过期、截图复用、尺寸或指标不符均失败。该脚本不会调用System Events、AppleScript或截取其他窗口。

仅调优查询时可以复用**已由真实导入产生的合成快照**，以避免每个SQL改动都重建百万事实：

```bash
python3 scripts/workbuddy_import_query_probe.py --phase dist/workbuddy-import-review/scale-1m-many-query-probe --output dist/workbuddy-import-review/query-1m-current
python3 scripts/workbuddy_import_faults.py --exe target/release/workbuddy-proxy --output dist/workbuddy-import-validation/os-faults
```

query-only报告区分原导入程序与当前查询程序哈希，不能替代当前程序的回填/资源/代理影响/布局轮次。最终仍以严格矩阵及全部其他关卡为准。
