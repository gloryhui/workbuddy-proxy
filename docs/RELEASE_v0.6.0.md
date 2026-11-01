# WorkBuddy Proxy v0.6.0 · Studio V2

Studio V2 带来统一的供应商、模型与统计界面，保留原有转发、协议、历史数据与主题设置。

- 新增 Studio Indigo 主题；供应商管理、用量概览、请求明细、汇总分析与统计设置采用新的布局、滚动和分页。
- 完善供应商和模型草稿、校验、复制与取消；外部配置更新保留正在编辑的内容，弹窗取消后不遗留空白遮罩。
- 修复统计数据库启动时短暂被锁定后持续停用，以及扫描恢复、重试状态不一致的问题。
- 改进长列表与未变页面的绘制缓存，修复 Windows 原生窗控和工具栏重叠、macOS 全屏时左上角背景错位。
- 统一 Core、设置界面、DMG 和 Windows 安装器的版本来源。

## 下载与升级

提供 Windows x64 安装包及便携 ZIP、macOS Apple Silicon 与 Intel DMG。使用本页附件和 `SHA256SUMS.txt` 校验下载文件。

升级前备份完整私有配置目录并保留旧安装包；详细步骤见仓库 `docs/studio_v2/UPGRADE_ROLLBACK.md`。历史六例 v0.5.6/v0.5.7 冷升级与回滚记录按原候选包来源保留。

## 验证范围与分发方式

冻结提交 `763f2ae` 的 [Core 检查](https://github.com/gloryhui/workbuddy-proxy/actions/runs/41296054368)与[三平台打包检查](https://github.com/gloryhui/workbuddy-proxy/actions/runs/41296054505)均已通过，包含安装/启动冒烟、配置和历史数据保护。Mac ARM 首轮一项锁等待时限断言出现耗时异常，保留原断言并仅复核该失败平台一次后通过；原记录仍可查。README 的五张 V2 图片为当前 0.6.0 程序的 macOS 原生实拍，采用隔离演示配置和合成记录。

以下专项本次未作完整验收：界面全状态及输入/焦点穷举、真实辅助技术、Intel 实机完整 GUI/托盘性能、Windows 物理输入法和托盘、多显示器与全部缩放比例、真实系统登录和现有 WorkBuddy 会话互操作，以及长期有效前台 CPU/内存峰值和启动性能。已有后台样本或自动化生命周期结果不替代这些专项结论。

沿用项目现有分发方式：macOS 为 ad-hoc 签名且未做 Apple 公证，Windows 包未签名。系统信任提示及企业设备策略以实际环境为准。
