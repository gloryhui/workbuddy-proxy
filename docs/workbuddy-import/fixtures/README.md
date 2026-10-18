# 合成输入

全部数据为设计人员编写，未复制用户聊天。固定时间为2026-10-16，本机fixture时钟必须固定在该日或之后。session-test.jsonl中两条重复表示应去重，conversationRequestId故意相同以证明不能按轮次去重。

expected.json是人工计算的黄金值；生产测试不得调用被测归一化函数生成expected。完整测试还需按ACCEPTANCE生成大行、坏行、同键冲突、断尾追加、权限/崩溃/数据库锁场景。

canary字符串 SYNTHETIC_PROMPT_MUST_NOT_PERSIST 只允许存在源fixture与测试断言中，不得存在数据库、API响应或日志。
