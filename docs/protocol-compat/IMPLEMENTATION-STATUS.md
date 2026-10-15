# 协议兼容实施状态

更新：2026-10-16。当前由 Codex 直接开发，dsh 不再参与。统计基线及固定分页已在本地 main 的 40ec917；协议实施 worktree 为 `/Users/glory/.codex/worktrees/upstream-protocols/workbuddy-proxy`，分支 `codex/upstream-protocols`。最终提交、验证和本地合并结果见同目录 FINAL-VALIDATION.md。

版本保持 0.5.3；管理协议升至 7，统计 API 仍为 2，SQLite schema 升至 4。没有推送、发布、安装、替换用户运行实例。旧阶段证据保留，但不再把“HTTP/发现/UI 待接线”作为当前状态。

## 已实现

- 显式四协议 Provider/Connection、旧配置默认、旧保存命令保留字段、复制/草稿/旧草稿继承兼容；不可识别协议和封闭设置校验失败。
- 路由冻结协议、Key、URL、profile；有效设置未变保留 generation，改变及 ABA 改变失效旧回放。品牌名称不改变协议。
- 三个原生请求编码、JSON/SSE 输出、语义终态、工具结果排序及原始 ID/opaque 回放。Responses 普通 assistant 文本使用 EasyInputMessage，缓存的原生项目保留完整原始结构。
- 真实 HTTP 转发及连接探测共用 prepare/open/decoder；原生鉴权/头白名单、前缀寻址、取消、背压、四并发、帧/输入/聚合/工具/缓存上限、安全错误。Chat 继续原字节转发，额外阻止客户端原生凭据 header 泄漏。
- 原生模型发现、完整匹配端点、分页/游标循环检查、合计 1 MiB / 4096 模型 / 16 页 / 30 秒限制；自动元数据刷新按协议鉴权。原生 live 能力按协议、基础地址、凭据指纹隔离；列表始终重新发现，不用其他账号的授权列表。
- WorkBuddy 能力与 profile/模型证据取交集，原生 useCustomProtocol=false。已知能力用于原生入场检查。目录解析、索引和持久化转移到阻塞池，JSON/写盘不持有热路径缓存锁。
- GPUI 协议选择、URL 提示、类型化预算/level 映射、默认输出上限/固定版本、草稿应用/保存、原生发现与模型选择、详情协议字段；1100×700 窗口的连接表单可滚动，保存/测试固定可见。
- schema 4 正式迁移及历史数据回归；同一统计 Guard 的协议字段、用量归一化和冻结价格。初始/部分 usage 不冒充最终输出账单；取消时输出/总量/总价保持未知。
- 无流量锁恢复须先持久化丢写计数再 Ready，正常关闭也保存计数。合法长标签/别名按统计列长度截取快照，稳定 ID/路由不改。
- CI 增加原生契约、发现、资源、统计与分页脚本；未推送，远端 CI 未运行。

## 证据与边界

核心和 GPUI 单测、严格 clippy、fmt、两个 Release 构建；独立真实核心 loopback 原生 JSON/SSE、图片、两轮同名并行工具、普通 assistant 历史、异常/取消/探测、统计价格和隐私；原生发现/缓存隔离/资源；旧功能回归及临时 app 核心与 helper 生命周期。明细见 FINAL-VALIDATION.md、acceptance.json、validation-2026-10-16。

已在隔离原生窗口检查协议选项校验、模型发现/勾选/保存、协议/profile 持久化，以及 1180×780 / 1100×700 布局。该证据不等于真实 WorkBuddy 与真实供应商端到端验收。

公开支持范围和限制以 [USAGE.md](USAGE.md)、[CONTRACTS.md](CONTRACTS.md) 为准。L6 实际供应商/WorkBuddy 验收单独记为未执行；本次不访问真实 Key，不安装测试版本。
