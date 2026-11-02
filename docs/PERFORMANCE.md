# 体积与性能 · v0.6.1

## 正式发行包体积

下表来自 v0.6.1 正式附件的文件字节数，与 v0.6.0 正式附件对比；收益包含系统字体、编译参数、图片依赖、导出和图标优化，不能全部归因于字体。

| 附件 | v0.6.0 字节 | v0.6.1 字节 | 减少 |
| --- | ---: | ---: | ---: |
| macOS ARM64 DMG | 38,865,857 | 13,281,771 | 65.83% |
| macOS x64 DMG | 40,263,870 | 14,344,758 | 64.37% |
| Windows x64 安装 EXE | 27,940,726 | 8,968,933 | 67.90% |
| Windows x64 ZIP | 33,827,484 | 9,905,711 | 70.72% |

[正式附件与 SHA256SUMS.txt](https://github.com/gloryhui/workbuddy-proxy/releases/tag/v0.6.1)。包体积下降不代表吞吐、启动时间或 RSS 同比例改善。

## 各平台程序大小

| 平台 | Proxy/Core 字节 | GPUI 设置界面字节 |
| --- | ---: | ---: |
| macOS ARM64 | 6,340,752 | 14,598,512 |
| macOS x64 | 6,989,408 | 16,362,160 |
| Windows x64 | 7,862,272 | 17,236,480 |

此处是解包后的程序大小，不是完整安装目录占用或进程内存。关闭设置窗口会释放 GPUI 进程，Core 和托盘继续运行。

## 测量与历史证据

编译参数 A/B 应保持源码、平台、依赖、构建模式和负载一致；图标、字体与编译收益分组对照。性能需报告样本、延迟分位数、吞吐和波动，不能用不同机器结果证明优化因果。RSS 合计可能重复计算共享页，GPU 内存须另测。本地模拟上游不能代表真实模型推理速度。

- [Proxy 编译与 GPUI 第二轮 A/B](https://github.com/gloryhui/workbuddy-proxy/blob/v0.6.1/docs/studio_v2/SIZE_OPTIMIZATION_ROUND2.md)
- [字体同条件 A/B](https://github.com/gloryhui/workbuddy-proxy/blob/v0.6.1/docs/studio_v2/ISSUE4_SYSTEM_FONTS.md)
- [旧版原始性能记录](https://github.com/gloryhui/workbuddy-proxy/tree/v0.6.1/docs/benchmarks)

历史报告中的本地构建目录可能已清理；保留下来的 Git 数据只证明记录当时的候选，不构成当前性能验收。v0.6.1 的验证和未覆盖范围见 [发布说明](RELEASE_v0.6.1.md)。
