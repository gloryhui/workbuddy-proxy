# 合成输入

全部数据为设计人员编写，未复制用户聊天。固定时间为2026-10-16，本机fixture时钟必须固定在该日或之后。session-test.jsonl中两条重复表示应去重，conversationRequestId故意相同以证明不能按轮次去重。

expected.json是人工计算的黄金值；生产测试不得调用被测归一化函数生成expected。完整测试还需按ACCEPTANCE生成大行、坏行、同键冲突、断尾追加、权限/崩溃/数据库锁场景。

canary字符串 SYNTHETIC_PROMPT_MUST_NOT_PERSIST 只允许存在源fixture与测试断言中，不得存在数据库、API响应或日志。

## 本目录新增样例（S01.04）

设计包 `docs/workbuddy-import/fixtures/` 的四个文件按字节复制到本目录（SHA256 与设计包相同），另加三个合成样例：

| 文件 | 场景 | 人工预期 |
|---|---|---|
| `session-split.jsonl` | 同一 `messageId` 的响应被拆成两个 `function_call` 表示，顶层 `id`/`timestamp` 不同、`conversationRequestId` 相同 | `expected-session-split.json`：1 条事实 + 1 次 duplicate，首 timestamp 冻结，不产生 conflict |
| `session-bad.jsonl` | JSON 语法错误、sessionId 与文件名不符、缺 messageId、非整数 timestamp、缺 requestModelId 各一行，另有 2 条合法对照行 | `expected-session-bad.json`：2 条事实 + 1 条 pending 候选 + 4 类固定诊断 |
| `session-tail.jsonl` | 前两行以换行结束，第三行是 342 字节的合法 JSON 但没有换行 | `expected-session-tail.json`：未追加换行前只提交前两行（offset 686），追加换行后第三行恰好入库一次 |

约定：

- 全部内容为合成数据，固定时间 `2026-10-16T00:00:00Z`（`1792108800000`），不含真实聊天、账号、密钥或文件路径。
- 每份 `expected-*.json` 的数值由人工按 fixture 行计算，**不得**调用被测归一化/解析函数生成。
- 需要大行（≥4 MiB）、UTF-8 损坏字节、权限错误、DB 锁等无法用纯文本样例表达的场景，由对应阶段的测试在一次性临时目录中合成，不放本目录。
- canary `SYNTHETIC_PROMPT_MUST_NOT_PERSIST` 只允许出现在源 fixture 与测试断言中。

## 接续补充：全字段实际导入

`session-fields.jsonl` 是独立合成场景，不修改冻结的 `session-test.jsonl` 或 `expected.json`。
人工预期：1条内置记录，输入100、输出20，缺少total时推导120；缓存读取40优先于低优先字段0，
缓存创建10，推理2，积分字面量`0.000000001`精确为1纳单位。缓存不重复加到输入，
HTTP/耗时/stream未知，无费用。`workbuddy_import_canonical.py` 用另一个隔离库经生产reader/writer、
列表/详情/总览验证这些字段；源中的canary不得进入目标库或API。
