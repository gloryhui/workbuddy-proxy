# WorkBuddy Proxy 文档 · v0.6.1

本目录仅维护当前正式版仍适用的说明和规范。旧发布清单、施工计划、交接记录、原始日志、历史截图和设计验证包已从当前目录清理；完整内容可在 [v0.6.1 固定快照](https://github.com/gloryhui/workbuddy-proxy/tree/v0.6.1/docs) 追溯。历史验收只代表当时的提交、环境和范围，不能替代新版本验证。

## 使用与维护

- [v0.6.1 发布说明](RELEASE_v0.6.1.md)
- [桌面应用](DESKTOP.md)、[设计与性能边界](ARCHITECTURE.md)
- [软件更新与恢复](AUTO-UPDATE.md)、[备份与冷回滚](studio_v2/UPGRADE_ROLLBACK.md)
- [供应商预设](PROVIDER-PRESETS.md)、[模型能力](MODEL-CAPABILITIES.md)、[模型同步与重启](WORKBUDDY-MODEL-REFRESH.md)
- [上游协议](protocol-compat/README.md)、[用量统计](statistics/README.md)、[WorkBuddy 用量导入](workbuddy-import/README.md)
- [体积、性能与测量边界](PERFORMANCE.md)

## 开发与设计

- [Java / Spring Boot 开发者源码导读](source-guide/README.md)：Rust 基础对照、服务核心、GPUI 界面核心、协议转换、统计持久化、用量定价、查询聚合与界面流转。
- [按风险验证的开发规范](CODEX_FAST_WORKFLOW.md)
- [Figma 设计交付契约](studio_v2/DESIGN_SPEC.md)、[画板与 Rust 映射](studio_v2/figma_node_map.md)
- [统计实现入口](statistics/DEVELOPMENT.md)
- [Windows 原生截图](WINDOWS-SCREENSHOTS.md)

`scripts/update_studio_progress.py` 是旧 Studio V2 施工台账生成器，只用于固定快照中的整套资料；当前文档不再生成旧版时间线。

## 许可

[GPUI 声明](GPUI-NOTICES.md)、[Apache 许可](LICENSE-APACHE-GPUI.txt)、[Magpie 许可](third-party/magpie-LICENSE.txt) 是打包输入，继续保留。运行包仅复制必要许可为 TXT，不打包 docs、Markdown 或历史截图。
