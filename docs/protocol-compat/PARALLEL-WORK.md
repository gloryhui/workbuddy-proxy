# 统计先行与协议开发协作边界

用户已明确另一个DeepSeek正在开发统计功能。当前协议任务只准备任务书，采用“统计先完成并合入、协议再实施”的顺序；协议业务代码不得从缺统计实现的旧基线开工。

## 启动门禁

1. 检查统计工作已有完成报告、测试和用户审定的整合结果。仅存在统计分支或任务书不算已合入；不得擅自替统计任务提交/合并来解除门禁。
2. 从用户审定目标分支读取包含统计实现的精确SHA，确认其包含统计已交付提交；检查实际统计命令/迁移/Guard，而不只读README。记录证据和SHA。
3. 有Codex桌面工具时先list_artifacts，再create_worktree显式ref=该SHA，使用唯一name如protocol-compat-impl；不可默认从可能滞后的远端分支创建。无工具时用安全Git命令新建独立worktree和唯一codex/protocol-compat-impl分支，不重置已存在分支。
4. 只复制本次docs/protocol-compat设计包到实施worktree，不复制整个脏目录，不stash主目录或任务书worktree。若同名设计已在新基线中，先比较并保留用户已确认的新版本。
5. 记录实施worktree、分支、SHA、管理协议版本、DB schema版本、统计基线测试结果。S0通过后才能改协议业务代码。
6. 统计尚未完成/合入时，报告依赖未满足，只继续整理任务书或无侵入审阅，不自动开启实现、不造统计空接口。等待用户适时启动，无需创建自动化。

## 目录隔离

- 任务书准备目录：通过 `git worktree list --porcelain` 核实的 `codex/protocol-compat` worktree，分支codex/protocol-compat；调研SHA为e1725c3d14c6dfed24cd4edc0150c1c052ab4bed。
- 正式实施目录：依赖门禁满足后新建，在IMPLEMENTATION-STATUS中登记；业务代码/编译/测试只在此目录。
- 统计目录：通过 `git worktree list --porcelain` 核实的 `codex/usage-statistics` worktree，分支codex/usage-statistics。不编辑、不执行其测试、不启停其进程、不切换/归档/清理该worktree。
- 主目录README与未跟踪docs/statistics不复制、不stash、不reset、不覆盖、不提交。
- 每次shell mutation显式指定实施worktree的workdir。使用独立临时config-dir/数据库/动态端口，只清理由自己创建且确认身份的PID；禁止按进程名全杀。
- 不自动向统计聊天发消息。有疑问记录待核对项，不代替用户调度另一个DeepSeek。

## 共享接口登记

统计合入后，协议开发可在自己的新worktree修改同名文件，必须保留已实现统计行为。每个共享入口登记文件/符号、旧/新契约、DTO字段、生命周期责任、相关两套回归。

| 入口 | 协议改造 | 必须保护的统计行为 |
| --- | --- | --- |
| relay_to/Body | 原生转换流 | 一请求一次Start/Finish，Body Drop结算和permit释放 |
| Destination/Route | protocol/URL/Key generation快照 | 供应商/模型/协议历史快照不随配置更名改变 |
| usage采集 | 原生类型化标准化事件 | 父子Token语义、缓存细项、缺失/未知，不能双计 |
| Bridge::test | 同真实适配路径、首事件即结束 | source=connection_test；探测通过不等于完整聊天 |
| Config/Draft/commands | 协议设置与原生发现 | 统计设置、commands、原字段和草稿指纹不丢失 |
| GUI/Job | 协议下拉和诊断 | 统计页面、查询与前台刷新不退化 |
| SQLite migration | 必要协议摘要字段 | 既有迁移、备份、事务、索引和历史NULL语义 |

## 管理协议版本

不要按旧调研基线直接把4改5。读取实际合入版本；如统计已升至5，则依据本次不兼容变化升到新未使用版本（例如6）。更新core/frontend/GPUI和旧UI旧core测试，验证旧程序不会覆盖新协议配置。不能因双方碰巧使用相同数字就宣称兼容。

## 交付与后续整合

本次不替统计任务合并/提交。协议开发从统计已整合基线开始，在独立分支交付；用户授权最终整合时再合入协议分支。最终同时运行统计新测试、协议新测试和全部旧回归，检查4并发/取消/usage单计数/DB失败不影响聊天/GUI页面与草稿。无Git冲突不代表语义兼容。
