# 协议兼容实施状态

2026-10-16：统计初版已审查、补齐并合入main，基线ad02658。当前实施worktree为upstream-protocols/workbuddy-proxy，分支codex/upstream-protocols。本次由Codex直接开发，dsh不再参与；提交/合入本地主分支已获用户授权，不推送/发布/安装。

用户追加界面要求：删除无操作作用的“导航”标签；一级依次“数据统计”“供应商管理”，供应商名称在管理项下面缩进；模型列表属于选中供应商的管理页。

待完成：配置与旧数据兼容、三种原生上游请求/JSON/SSE/工具回传/图片/思考签名/安全错误、统一统计协议字段迁移、协议选择/发现/测试/能力交集、独立Mock和回归/性能、主分支最终合并验证。旧任务书为设计草稿；若与当前实现/授权冲突，按最新用户要求和核实后的官方契约修订。

用户追加性能要求：成本定价、采集与存储供应商模式均为固定页窗口（20/50/100），不能加载更多后无限叠加；统计明细/分组继续服务端游标分页。

本地main已前移到40ec917：上述固定分页和侧栏层级已合入。价格321条真实核心接口的20/50/100前后遍历、末页和精确筛选通过；54个GPUI测试、严格clippy、fmt、debug构建通过。原生窗口核对侧栏及默认价格页仅20组行操作；未声称所有页大小均完成原生人工验收。证据位于docs/statistics/validation-2026-10-16/fixed-pagination.txt。没有推送/发布/安装。

协议实现进行中，尚未对外启用：src/protocol/{config,request,encode,sse,replay}.rs已建立独立模块，main暂时标注dead_code允许分阶段接线。19个协议模块测试通过，包含四种协议枚举/封闭配置、网关前缀与端点校验、严格JSON/重复键/深度、图片base64校验、同名并行工具的ID历史校验、三协议请求编码和opaque回放接口、显式思考profile与Responses结构化输出、有界SSE解析及任意字节边界/UTF8/BOM/CRLF/超限。该结果不代表已完成原生转发或真实工具闭环。

已补进程内ReplayCache：30分钟TTL、512工具ID/32MiB保守分配量预算、单项1MiB/响应8MiB、128bit随机wbpt ID、route与前序历史/assistant文本及工具绑定；pending未提交不可见，合法终态后提交，读取可重复用于重试/分支。新增3个测试覆盖缓存取消/重试、上下文篡改、route失效、过期/驱逐/大小预算。严格核心clippy通过。

下一步：原生JSON/SSE输出状态机、usage规范化；再贯穿Provider→Connection→Routes→管理命令/发现/测试/GPUI，接现有统计Guard并正式迁移upstream_protocol列。协议暂未写入正式配置，旧Chat路径行为未改变。最后独立Mock/回归/性能并合入main，全部通过才结束goal。

需继续核对：Gemini结构化输出文档字段正在变化，当前明确拒绝非text response_format，不删除schema；Gemini strict=true工具约束、parallel_tool_calls=false也明确拒绝。schema暂为已列明的保守子集。原生并行工具结果已按调用顺序重排，保留ID和结果字符串关联。profiles的预算默认不猜值，max需显式映射。
