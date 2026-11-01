# 界面截图

## 当前 README：Studio V2 v0.6.0（2026-11-01）

当前五张图来自 macOS Apple Silicon 上运行的 v0.6.0 Studio V2 原生应用，原始 JPEG 均为 1440 × 901 像素；没有裁剪、重绘、拼接或图片转码。设置界面运行源码为 `85540e1`（含全屏背景修复），Core 为 `dd9deba`，后续发布准备只更新文档和截图。

| 文件 | 内容 |
| --- | --- |
| [studio-v2-providers-20261101.jpg](studio-v2-providers-20261101.jpg) | 供应商连接、模型目录、本地代理与活动日志 |
| [studio-v2-overview-20261101.jpg](studio-v2-overview-20261101.jpg) | 用量、请求量、缓存命中、趋势与费用 |
| [studio-v2-usage-20261101.jpg](studio-v2-usage-20261101.jpg) | 请求明细、Token、缓存、费用与固定分页 |
| [studio-v2-summary-20261101.jpg](studio-v2-summary-20261101.jpg) | 供应商分组、Token 构成与缓存覆盖 |
| [studio-v2-skins-20261101.jpg](studio-v2-skins-20261101.jpg) | Studio Indigo 与原有六套皮肤 |

使用独立临时配置和隔离环境，演示 Key 始终掩码；12 条请求由本机 loopback 模拟上游产生，费用、Token、速率和成功率均为合成示例，不代表真实账单、服务表现或报价。12 条请求位于同一日，趋势如实显示单点。没有读取真实 WorkBuddy 配置、密钥或调用历史。

[采集与哈希记录](../studio_v2/benchmarks/release94/native-capture.json)保留了原始拍摄动作和来源。截图不是 Windows 实机或 Intel 硬件验收证据。

## 历史截图：Windows v0.5.7（2026-10-21）

- 应用版本号：0.5.6；拍摄时仓库提交为 `49d5d7c`，应用代码对应 `d43f17c`，两者之间仅有 README 文字更新。
- 来源：当前 main 的本地 Windows 构建，包含皮肤设置、统计布局、独立工具收费和趋势缺失区间等后续改进。不是 GitHub v0.5.6 正式安装附件的界面记录。
- 平台：Windows 10 专业版 22H2（19045），默认黑白浅色皮肤。
- 拍摄方式：Windows GDI（Pillow ImageGrab），通过演示进程 PID 限定目标并裁取原生窗口边界；PNG 原始像素，未重绘、拼接或修改界面内容。
- 后续 Windows 截图优先复用这一方式：[操作步骤与限制](../WINDOWS-SCREENSHOTS.md)，[截图脚本](../../scripts/windows_screenshot.py)。
- 隔离：独立临时配置与 HOME / USERPROFILE / APPDATA / LOCALAPPDATA，演示 Key、loopback 地址、24 条合成代理记录及 2 条合成图片生成收费记录。未读取或展示真实密钥、账号配置或调用历史。
- 数据示例：26 条记录，工具积分合计 6.4；七天趋势故意留出一天无记录，演示缺失区间虚线。Token、费用、积分及速率均为合成示例，不代表真实账单、服务表现或官方报价。

| 文件 | 内容 | PNG 尺寸 |
| --- | --- | --- |
| [windows-providers-20261021.png](windows-providers-20261021.png) | 供应商设置、协议与模型列表、全局设置、测试与保存按钮 | 1426 × 893 |
| [windows-overview-20261021.png](windows-overview-20261021.png) | 七天概览、Token 与积分、用量与费用趋势、缺失日期虚线 | 1426 × 1093 |
| [windows-usage-details-20261021.png](windows-usage-details-20261021.png) | 明细与固定分页、ImageGen 工具名称和标签、Token 不适用 | 1426 × 1093 |
| [windows-summary-20261021.png](windows-summary-20261021.png) | 供应商分组统计、模型 Token 和费用、独立收费积分 | 1426 × 893 |
| [windows-skins-20261021.png](windows-skins-20261021.png) | 衣服图标入口、默认黑白及另外五套配色 | 1426 × 893 |

演示窗口分别使用 1440 × 900 / 1440 × 1100 的外部尺寸；上表为去掉 Windows 不可见边框后实际截取的原生窗口尺寸。较高窗口用于完整展示概览图表或十条明细。

## 历史截图：macOS v0.5.5（2026-10-16）

- 版本：WorkBuddy Proxy v0.5.5，Release 构建的应用源码提交 `fe4cc50`；后续至 `862fc98` 的变更仅涉及测试和 CI。
- 拍摄日期：2026-10-16。
- 平台：macOS Apple Silicon，1440 × 900 逻辑像素窗口，Retina 截图 2880 × 1802 像素（含标题栏）。不作为 Windows 实机截图。
- `statistics-zh-v0.5.5.jpg`：统计总览、七天筛选、Token 汇总、缓存命中率、费用和趋势。
- `request-log-zh-v0.5.5.jpg`：向下滚动后的请求日志、分页、供应商 / 模型分列、Token 明细、费用与 tok/s。
- `macos-zh-v0.5.5.jpg`：供应商设置、上游协议、模型列表、WorkBuddy 同步和本地代理状态。
- `model-discovery-zh-v0.5.5.jpg`：从本机模拟服务获取模型，显示已有模型与待添加项。
- `macos-en-v0.5.5.jpg`：通过地球图标切换 English；演示供应商名称不会自动翻译。

以上历史图片来自 v0.5.5 Release 二进制的原生 GPUI 界面，使用独立临时 HOME、演示 Key 和 loopback 模拟上游，没有读取或展示真实 WorkBuddy 配置、密钥与调用历史。统计数据库仅在临时配置目录中填入合成记录和价格，金额及速率不代表真实服务表现或官方报价。截图仅转为 JPEG 压缩，未重绘或拼接界面。
