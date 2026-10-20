> **最新用户价格决定覆盖本轮旧价格要求**：先读[PRICING-DECISION.md](PRICING-DECISION.md)和[PRICING-CHANGE-PROMPT.md](PRICING-CHANGE-PROMPT.md)，当前价可改/可删、历史逐调用快照、缺价默认0、停用历史重算。其他F项继续。

# 使用统计：复核后修复推进 Prompt

将下面分隔线内全文交给DeepSeek。它需要能读取本文件、审查报告和原设计包。

---

请继续修复 WorkBuddy Proxy 的使用统计实现。本轮目标是修复审查确认的正确性问题、补齐原需求缺项并提供验证证据，不是重写项目，也不是仅解释为什么现有实现可以接受。

## 1. 工作区和必读资料

主工作区：主仓库（通过 `git worktree list --porcelain` 核实）。

继续使用已有独立worktree：统计开发 worktree（通过 `git worktree list --porcelain` 核实 `codex/usage-statistics` 对应目录），分支`codex/usage-statistics`。先检查实际HEAD/status/worktree列表及生效AGENTS.md。审查时HEAD=e1725c3，功能代码均未提交；保留全部已有实现和用户修改，不reset、不stash、不重新复制整个主工作区，不创建相互冲突的第二套实现。

**先读取最新审查资料（在主工作区，开发worktree原副本没有这两份新文件）：**

- `docs/statistics/REVIEW-2026-10-15.md`
- `docs/statistics/NEXT-ITERATION-PROMPT.md`
- `docs/statistics/review-2026-10-15/`

必要时只复制这些审查资料到当前开发worktree，不覆盖原设计/现有源码。再读开发worktree中的REQUIREMENTS.md、API.md、DEVELOPMENT.md、IMPLEMENTATION-STATUS.md及实际源码。

开始就把IMPLEMENTATION-STATUS中的笼统“完成”改为准确的“复核修复中”，列出F01–F11和验证矩阵。现有86核心单元、30 GPUI单元、33集成通过只作为基线，不能证明下列漏测场景已修复。

## 2. 第一批：F01/F02/F03，先写失败回归再修核心

### F01 usage合并

新增回归：第一事件`{"prompt_tokens":100}`，第二事件`{"prompt_tokens":200,"completion_tokens":10,"total_tokens":210}`。当前会合成input=100/output=10/total=210的Complete，违反SQL约束并影响整批。

逐字段检查双方已知值是否冲突；仅缺失字段可补齐；合并后再次验证total/父项/细分。错误usage记invalid摘要且不携带非法数值，不得拖垮其他请求。补充“同样父项，第二事件仅新增合法cache细分”应可兼容，而非invalid。重复完整相同usage仍只计一次。

### F02 故障恢复

新增真实临时SQLite锁回归：Start已提交→持锁→请求终结→超过有限重试→释放锁→新请求。要求最终自动恢复ready、后续继续记录、原孤立行有可信终态、失败区间有degraded/unknown证据。

当前degraded关闭accepting，而ready只靠后续事件成功恢复，事件队列空时会停采到重启；修复这条闭环，不能用无限重试、无限缓存或让聊天等待数据库。

对批失败隔离确定性坏事件与暂时性DB故障，有限重试/探测；按请求记录丢弃与写失败；处理Start丢失后的Finish和已Start未Finish孤立行。保持原始转发fail-open。持久化采集质量区间，故障期间不能继续宣称collecting/真实零。确保恢复/关闭流程都能在预算内结束。

### F03 价格单位

严格恢复API.md：`*_rate_nanos`是“纳货币单位/百万Token”的整数十进制字符串。`"2000000000"`表示$2/M，`"1"`表示1纳单位，不是1美元。

API save/list/detail使用一致单位，UI金额表单用定点整数解析做一次转换，显示反向转换；不要用f64。现有SQLite规则已经是纳单位，**禁止对数据库整体乘除1e9**。若需兼容旧格式必须显式版本/字段，不能同字段猜单位。

补价格往返、零、0.2、2、边界、非法字符串、小数纳值拒绝、单位上限、分项金额及列表显示测试。更新进度里“文档单位未定义”的错误解释。

第一批完成后跑针对性回归+核心test/clippy，再进入后续批次。

## 3. 第二批：F04–F09，接口与统计一致性

### F04 游标和完整翻页

- reader实际执行时验证cursor.run_id/filter_key/maintenance_revision和非负字段范围，过期返回stats_cursor_expired，不能只生成字段不检查。
- 测试修改任一身份字段、换filter、正常重启、维护后使用旧cursor，均按契约失效；合法cursor完整遍历不重不漏。
- GUI保留服务端next_cursor，不用末行另造空run_id的cursor；冻结一次查询的时间/时区/filter，dashboard和rows/翻页共用，不能每次filter()重算now。
- 上一页使用pop出的上一页起始cursor，覆盖1→2→3→2→1。请求与分组分页分开；使用服务端has_more/next_cursor，不能仅以rows.len==page_size猜测还有下一页。
- 接上20/50/100页大小、分组下一页、维护/重启失效后的明确重置提示。

### F05 SSE分块不变性

当前Collector把chunk.len算入驻留缓存，195098字节合法SSE整块输入会丢首usage，分4096字节输入却Complete。

限制真实单事件/驻留状态，增量扫描大chunk；同一合法字节序列按1字节、4096字节、随机分块、整块输入结果一致。保留超大事件丢弃后恢复、UTF8/CRLF/多data行和原始响应字节透传。禁止collect全部响应解决问题。

### F06 终态判定

非流式JSON先完成有界EOF解析，再决定outcome；HTTP200错误对象记failed，畸形JSON不能completed。finish和Drop仍只结算一次，普通JSON成功和取消/编码场景不回归，HTTP/body保持原样。

### F07 费用状态和历史快照（新业务替代原重算要求）

执行PRICING-DECISION.md：移除当前价覆盖历史重算，逐请求保存单价/金额；缺价默认0。not_applicable、缺用量/拆分、overflow等原本可验证的状态仍正确呈现。历史查询不关联当前价格，不再将缺价排除出金额统计。

### F08 退出顺序

在stop/Ctrl-C/正常退出销毁网络runtime之前mark_closing并停止接纳，Guard销毁后再flush。慢流正常停服记录interrupted/shutdown，真实客户端取消仍cancelled/client_drop。关闭等待上限不能被阻塞control.send绕过，检查迁移失败/队列满/DB锁下的退出。

### F09 真实零与未知

在可证明collecting覆盖且无丢失的空桶，主input/output/total返回0；无覆盖或故障期间保持null。请求存在但usage缺失仍null，有部分完整usage仅汇总可信部分。补健康空桶、暂停空桶、故障空桶、有请求无usage四组API及图表转换测试。

## 4. 第三批：F10/F11，补齐GPUI交互

### F10 可靠的任务状态机

- Job/Reply明确区分Kind、唯一seq、filter_generation、tab和cursor。切页签、改条件立即使旧请求失效，不能在debounce期应用旧回复；过期回复不能清掉新pending。
- 模拟慢dashboard、先后颠倒rows/分组、切页签、刷新、离开页面、失败/迟到回复，验证旧内容不会覆盖新条件或被静默反序列化失败吞掉。
- 有界队列满时保留最新只读意图等待重试；设置/清理/定价等写操作不能静默丢弃。不要一次向4槽队列塞100个供应商模式任务然后忽略结果。
- 保存完整HealthDto，状态徽标以ready/disabled/degraded/migrating和loss_scope为依据，查询成功不代表采集健康；展示真实DB大小/最早记录/故障和暂停提示。
- 仅前台统计页且无冲突模态时自动刷新；离开页面停止debounce发新请求。维护轮询改为2秒、独立in-flight去重，不每250ms重复排队。
- 补调度层/控制器测试。已有30项GPUI测试主要是工具函数，不能替代页面状态机验证。

### F11 P1表单与筛选（价格部分以新口径为准）

- 价格对话框有独立供应商、上游模型、币种、模式、四项价格字段及直接编辑/删除；不在未选择时静默把规则归到首供应商。
- disjoint有cache_write输入。允许直接修改/删除当前配置，历史快照不变；撤销生效区间和引用锁定。
- 保存等待成功再关闭；失败保留输入在对话框内纠错，save/list用不同JobKind/DTO，成功刷新规则和相关统计。
- 自定义日期按原需求实现，明确from/to和本机时区。筛选选项支持next_key/搜索，不限于前100个；模型按当前供应商收窄。
- 语言切换刷新已创建Select选项/提示/对话框，保留选择和未保存模型草稿。
- 保留原生GPUI Kit 0.7.3和现有主题/图表，不为了修状态机改成WebView或推倒重做全部UI。

## 5. 验证和交付

每一批：先写能抓到旧缺陷的回归，再修复，记录实际输出。不能仅修改测试预期迎合错误代码，不能删设计要求来宣布完成。

最终至少执行：

1. 设计SQL验证；核心与GPUI fmt/test/clippy；release构建；原33项statistics_test加本轮回归。
2. 现有smoke、multi_routing、connection_test、core_compatibility、model_capabilities及适用回归。
3. 重新检查原始转发字节、取消许可释放、关闭设置后继续采集、未保存草稿、多供应商身份和隐私边界。
4. 10万/100万数据查询预算、额外内存、采集开关前后转发P95；特别测已有价格/缺价默认0/全为unsupported三类，而不只测无价格快路径。
5. GPUI三尺寸、中英文、三页前后翻、切条件/页签竞态、采集故障、价格失败再成功、维护进度等实际检查；屏幕权限受限明确保留人工项，同时完成可运行的状态机测试。
6. Rust1.88最低版本与Windows平台证据分别列出；无法验证就标未验证，Mac/Rust1.98通过不能替代。

每批更新IMPLEMENTATION-STATUS：F编号、修改文件、测试命令/结果、未验证项，保留原始失败证据。最后提交F01–F11逐项闭环表和真实git status，不把部分完成或Mock当完整验收。

本轮不授权push、release、安装、改真实用户数据库或清理历史。提交须遵守实际授权与AGENTS署名条件，不把其他AI工具贡献冒充Codex，不为补署名改历史。

现在从第一批开始，并持续推进全部修复；只有真实阻塞才提出缺失信息，不要每批结束询问是否继续。

---
