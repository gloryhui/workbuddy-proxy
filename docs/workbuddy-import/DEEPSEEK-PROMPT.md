# 交给 DeepSeek 的执行指令

你负责按已经冻结的设计实现 WorkBuddy 本地 JSONL 用量导入。本任务不需要你重新设计产品或决定架构。先完整读取本目录 README、DESIGN、DATA-API、UI-SPEC、TASKS、ACCEPTANCE、COMMANDS、PROGRESS，以及worktree根AGENTS.md；任何局部摘要都不能替代契约。

工作目录使用 `git worktree list --porcelain` 核实的本机导入开发 worktree，不要求固定绝对路径；分支 `codex/workbuddy-usage-import`，基线 `6102b779871bab0bb8ad646dac5004892adf76be`。不要在原main目录写代码，不创建另一套临时实现，不合入自动更新等无关分支。开始前执行只读Git核验，保护已有设计文件。

严格依次执行 S01.01 → S01.02 → … → S10.08 共80项。每次只处理一个子任务，完成该项修改、测试、范围自查和证据记录后才能处理下一项；每8项做阶段关卡。不得跳过schema/解析测试直接写UI；不得一次改完所有文件再补勾任务。

每项输出固定格式：

```text
任务：Sxx.yy / ACxxx
当前前置：已通过哪些证据
本项允许文件：...
实际修改：...
验证：精确命令 + 退出码 + 测试数量 + 证据路径
结果：PASS / FAIL / BLOCKED_EXTERNAL / BLOCKED_DESIGN
下一项：编号（仅当前关卡已通过才能开始）
```

你必须遵守：

1. Proxy SQLite记录是代理调用权威来源；镜像JSONL不能再累加，缺失代理记录也不自动用JSONL补。
2. 全部报表查询SQLite；JSONL由后台worker增量导入，不能在UI/转发线程同步扫描。
3. 一个事实表加检查点/身份账本/历史归属；去重、清除不复活、事务+ACK、归属不确定不计入均为硬要求。
4. 固定三个图标置筛选条最前：全部来源、内置模型、供应商模型；不能增加第四个来源按钮或自创新导航。用户直连与代理用高级采集方式区分。
5. 图标/宽高/换行/状态/中英文按UI-SPEC，不能只写“优化布局”然后自由发挥；默认日期仍当天。
6. 不伪造HTTP成功、耗时0、费用0、完整性100%；导入点数与货币分开。
7. Rust typed模块、Option未知、整数纳单位、参数绑定、bounded queue、单writer、可重放状态机；禁止unwrap外部输入、silent fallback、无界内存、重复实现生产逻辑的测试。
8. 不增加依赖/更新Cargo.lock/修改旧迁移/版本号/上游转发协议/供应商配置含义；允许文件边界以TASKS为准。超过边界先写DECISIONS证据，不能擅自执行变更。
9. 不删/改弱既有测试，不加allow来规避Clippy，不把0测试/跳过平台/Mock成功说成实际验收。
10. 只使用合成数据和隔离HOME/配置；不写真实WorkBuddy目录、真实Proxy库，不重启用户WorkBuddy，不联网获取账单或登录凭据。
11. 文档的命令、SQL和fixture都是待实施契约，不是已经实现的事实；validate_spec通过仅代表设计校验，不允许直接完成S01或业务验收。
12. 完成每项必须更新PROGRESS并建立evidence；阶段失败修本阶段，不标DONE后继续。

遇到未知格式、平台能力缺失、现有API与设计不一致时：记录准确证据，提出最小变更，标BLOCKED_DESIGN/EXTERNAL；禁止通过猜测、降低断言、显示假数据来绕过。不要重复请求常规实现许可，用户已授权此功能的工作范围；只有确实需要变更冻结契约时请求决定。

本任务未授权提交/推送/合并/发布。若用户之后要求提交，按实际参与者处理署名：设计文档由Codex参与；DeepSeek编写的代码不能冒充Codex贡献。保持真实Author/Committer，禁止模型名替代作者，Codex实际贡献所在提交遵循根AGENTS精确trailer且只一次。不要为补署名改历史或安装Git Hooks。

现在从 S01.01 开始，先核验worktree和基线，再报告该项结果并按固定顺序继续。

## 施工中修订1.1

优先读取DECISIONS.md的WB-RT-001：后台固定5秒增量/新文件发现与页面刷新完全独立；禁止新增实时选项。若已开工，从当前子任务安全边界接入，按变更记录补验，不重新从S01开始、不重置PROGRESS。
