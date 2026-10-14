# 性能测量与历史记录

## v0.5.0 正式版

来源：[主分支发布构建](https://github.com/gloryhui/workbuddy-proxy/actions/runs/38558550741)，提交 `510e853fdc685f42477307d8fa9c66de2032fbd5`。配置为 1 个供应商、64 个模型。

| CI 环境 | 单独核心空闲 RSS | 设置打开：核心 + 托盘 + 界面 | 设置关闭：核心 + 托盘 |
| --- | ---: | ---: | ---: |
| Windows x64 | 9.23 MiB | 77.54 MiB | 23.31 MiB |
| Mac Apple Silicon | 11.83 MiB | 147.60 MiB | 51.73 MiB |
| Mac Intel | 4.58 MiB | 71.13 MiB | 20.62 MiB |

核心空闲值与界面开/关值来自不同的隔离测试进程，不能相减推断单个组件的精确占用。RSS 合计可能重复计算共享页面，GPU 内存未单独测量，CI 机器与物理设备存在差异。

- `gpui-*.json`：真实发行包的核心、托盘、设置进程生命周期和 RSS。关闭窗口释放设置进程，保留两个后台进程。
- `core-*.json`：无界面核心；直连与代理各 200 个小请求，4 并发、80 个约 1 MiB 请求，记录采样峰值和负载后 RSS。
- 转发测试使用本机 HTTP 模拟上游，客户端每次新建连接，上游连接池复用；中位数差值出现负数不代表代理没有成本，不能作为真实模型速度或跨平台排名。

原始 JSON 保存在 [benchmarks/v0.5.0](benchmarks/v0.5.0/)。本版发行包为 Windows 安装 EXE 7.00 MiB、Mac ARM DMG 13.67 MiB、Mac Intel DMG 14.45 MiB。

## 历史原生版记录

下方记录属于已经归档的旧设置界面，不代表当前 GPUI 版本。旧源码保存在 [backup 分支](https://github.com/gloryhui/workbuddy-proxy/tree/backup)。

数据来自 `scripts/benchmark.py`。它们是过程测量，**不是全网最小内存或最快速度排名**。每个 JSON 保留平台、模式、样本数、二进制大小和测试边界。

| 测量 | Mac ARM64 本机 | Windows x64 CI |
| --- | ---: | ---: |
| 无界面核心空闲 RSS | 6.23 MiB | 8.61 MiB |
| 设置界面打开时核心 RSS | 6.22 MiB | 8.68 MiB |
| 界面 + 核心进程 RSS 和 | 78.22 MiB | 26.07 MiB |
| 4 并发、1 MiB 输入的核心采样峰值 | 35.41 MiB | 11.72 MiB |
| 核心负载结束后 RSS | 35.41 MiB | 10.05 MiB |
| 小请求代理本地 p50 | 0.189 ms | 15.733 ms |
| 小请求代理本地 p95 | 0.264 ms | 16.111 ms |

Mac 数据取自本机 ARM64 发行构建。Windows 数据取自 GitHub Actions 运行 [38376533657](https://github.com/gloryhui/workbuddy-proxy/actions/runs/38376533657)，代码提交 `58cb754`。后续核心进程隔离修复不改变转发热路径，仍需以对应 CI 原始数据为准。

Mac / Windows 的 RSS 与 Working Set 含义不完全相同，CI 机器与本机硬件、调度器、Python mock 上游也不同，**不能根据该表推断某个平台本身更快**。客户端每次建立新连接，代理复用上游连接，代理 p50 可能低于直接请求；这不能证明代理没有开销或能让真实模型推理更快。

首版 v0.1.0 的设置窗口按需存在。v0.1.1 起 Windows 关闭 / 最小化保留托盘与 UI 进程，因此正常托盘模式占用应参考“原生界面总 RSS”；Mac 仍在关闭窗口时释放 UI 进程。核心继续工作，恢复窗口仍使用原来的核心 PID。进程 RSS 和可能把共享页面重复计算，但不会遗漏 UI 进程。Mac AppKit 初始化显著高于核心，因此没有把“后台 6.23 MiB”当成界面打开时的总占用。

1 MiB 输入负载后，分配器和连接缓冲可能保留内存，所以负载结束的 RSS 不一定立即回到启动值。8 MiB 输入上限和 4 并发是逻辑工作预算，不是操作系统 RSS 上限；短时峰值也可能被 150 ms 采样遗漏。

本表使用本地 HTTP mock，未覆盖 TLS 握手、公网延迟、上游限流或模型思考。另做的真实 OpenCode Go / 原生 TLS 流式连接测试返回 HTTP 200；它仅证明该请求可以连通，不是全面 WorkBuddy 人工验收。

原始记录位于 [benchmarks](benchmarks/)。每次 CI 的最新测量也保存在该次 Actions 构建附件中。
