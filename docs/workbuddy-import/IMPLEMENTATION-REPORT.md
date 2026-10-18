# WorkBuddy JSONL用量导入：当前实现与验收

## 2026-10-19 用户反馈修复：自动分类与本机代理优先去重

本轮模型归类仅内置／自定义，自定义日志无需确认创建者。旧模型层级的代理排除规则退出用量判定。请求头与JSONL的会话／消息标识仅存哈希，用于逐次调用去重；两种到达顺序均保留原始代理明细。撤下副本以128条为界分批提交，持久任务可重启续做。0001～0007未改，0008添加关联索引并把旧排除项安排后台重读。应用仍0.5.6，schema8/API4/control12。

核心551项、UI137项、格式和严格Clippy通过；隔离v7排除92条→v8恢复、复制文件／重扫／重启、两种到达顺序及130命名空间分批续做均验证。按当前1440×900远程界面检查分类筛选、汇总、明细、进度和说明。真实数据库／原应用一致性备份后，已安装匹配核心与UI，92条自定义用量恢复，原49条内置事实逐行哈希不变；共141条。恢复的92条共5,239,250 Token，原始积分缺失保持未知。真实UI“近7天→自定义”92条和明细已验证；重启稳定。

本轮验证进程已停止、专用临时目录已删除，至少释放46MiB；未知归属的系统临时文件保留，未按前缀删除。源码未提交／推送／合并／公开发布。详细证据见[反馈修复收尾](../../evidence/POST-ACCEPTANCE-20261019-TWO-MODEL-CLOSEOUT.json)。此前大规模／平台性能证据保持各自历史源码范围，本轮未重跑全部百万行矩阵。


基线 `6102b779871bab0bb8ad646dac5004892adf76be`，分支 `codex/workbuddy-usage-import`，worktree `/Volumes/1TB/codex/worktrees/workbuddy-usage-import/workbuddy-proxy`。版本仍为0.5.6；清单、锁文件及迁移0001～0006保持基线字节。未提交、推送、合并、安装或发布。

## 当前本机验收范围（2026-10-18）

当前ARM核心`bcaa0836…`、UI`8f476d71…`；x64核心`0d81bb3e…`、UI`d9d960a3…`。核心553/UI139命名测试、fmt/Clippy/release和[27实际运行组](../../evidence/R09-20261018-MAPPED-READER-CORE-GATES.json)通过，原545核心测试名称全部保留。版本/依赖、迁移0001～0006与HEAD未变；未提交、推送、合并、安装或发布。

[完整12阶段矩阵](../../evidence/R09-20261018-MAPPED-READER-FULL-MATRIX.json)已通过：100k/1M×单/多文件×3，2298项含隔离断言及全部严格SQLite快照，六组跨规模资源增长核对。百万单文件回填三次约602～618秒，多文件约436～447秒，仅为本机SSD实测；按用户要求取消600秒全导完门槛，不推断机械硬盘吞吐。每阶段20次正常增量逐次≤10秒、查询各轮P95≤500ms、代理4并发开关对照均达标。矩阵中另有一次300秒空闲实测（不是12次），正文读取为0且CPU中位数达标。[百万状态轮询13项](../../evidence/R09-20261018-MAPPED-READER-STATUS-POLLING.json)也已通过。

历史回填在后台进行，部分已提交统计可查；进度按已发现文件检查点字节计算，展示剩余文件数、剩余数据量，另列已入库条数，不为计数额外扫JSONL。发现未知、完整核对不定进度、回填/暂停/错误重试恢复及追平后固定5秒增量，均有当前[18项检查点探针](../../evidence/R09-20261018-MAPPED-READER-CORE-GATES.json)与[原生配对](../../evidence/R09-20261018-MAPPED-READER-NATIVE.json)证据。

原生完成3来源×2语言×2窗口的12矩阵，实际20万有效行回填、键盘/输入、归属过期/CAS/确认/撤销、分页/窄窗详情滚动、100次普通刷新正文读数不变、7个刷新/隐藏场景最慢5285.134ms。28事件样本零丢弃，12／12下一帧选择匹配，最长同步段15.555ms。仅覆盖当前远程桌面分辨率；GUI/tray只有唯一QA身份与完整私有环境，headless core有实际PID kernel证明。

[候选ARM实际镜像及驻留退出](../../evidence/R09-20261018-MAPPED-READER-PACKAGE-NATIVE.json)通过：1M／1GiB未回填完时真实Cmd+Q，观测约417.927ms内原设置/托盘/核心全部退出；完整两次启动/退出检查约2.310秒，两次持久原因normal。该案例的进程退出观测与独立UI事件trace分开记录。[x64本机Rosetta构建/运行](../../evidence/R09-20261018-MAPPED-READER-MACOS-X64.json)通过；不声称物理Intel测试。

4／4实际百万断点续传组合均通过，80AC本机范围79 PASS／1 BLOCKED_EXTERNAL、47X全部PASS。

正常退出与SIGKILL独立验收；每案真实唯一百万行／至少1GiB，无SQL种子，重启不重写配置/源文件，用真实冷启动反例及早期提交receipt证明seek/跳过已完成文件，最终精确核对事实/账本/Token/积分及EOF/代/运行终态。原v1误把预读量当作整文件重放，失败及[读量诊断](../../evidence/R09-20261018-RESUME-READ-AHEAD-DIAGNOSIS.json)保留，未改成PASS。

[80AC](ACCEPTANCE-AUDIT.md)和[47X](CROSS-CUTTING-AUDIT.md)逐项列出条件、当前命名测试/真实检查及证据。Linux/Windows执行和Windows GUI、物理Intel、OS挂载/安装均未完成，明确保留外部边界，不能称全平台完成。候选镜像已验证代码身份/生命周期；最终ARM/x64文档镜像须独立核对每份文档SHA、签名/代码身份和保护下生命周期，实际结果由`dist/workbuddy-import-review/mapped-reader-final-{arm,x64}-package-archive-coordinator.json`及[CURRENT入口](../../evidence/R09-20261017-CURRENT.json)记录，不以打包代替安装。

所有历史失败、原候选哈希及原始报告保留。临时目录固定外置盘，复用已有Cargo缓存；每轮停等本轮子进程并确认无引用后只删除确切自有路径，报告清理/残留/系统盘余量。

## 历史候选记录（以下“当前/待验/未完成”均指当时哈希）

## 5498候选有限让行验证（已失败，保留历史范围）

当前待验ARM核心为 `54989a654cc8a319e6d75bb2f5c9c3555e17da6012b8393158837f5506d0a7aa`，545项核心测试、fmt、Clippy及release通过，详见[构建证据](../../evidence/R09-20261018-READER-YIELD-BUILD.json)。UI源码与既有8f4/d9d9程序未改变。[27组串行运行门禁](../../evidence/R09-20261018-READER-YIELD-CORE-GATES.json)全部通过。两路客户端完成46,492次真实非空dashboard请求且无失败，20次精确身份增量窗口均≤10秒，历史回填同时推进；这不替代百万查询百分位门槛。当前正在复测百万多文件失败场景，通过才续完整12阶段。

810f候选在约514,304条已提交时的后台dashboard超过原2秒预算；失败快照为521,984条。停止导入写入后，同快照真实API首次约670ms、随后约176～184ms，支持写入竞争是当前瓶颈的判断。新策略用真实reader生命周期计数，让报表执行时导入最多让行400ms，持续报表仍定期获得一次正常有界导入poll。控制、代理事件和退出维持原优先级；没有增加报表超时或放宽P95≤500ms、增量≤10秒等门槛。实际改善及持续查询不饿死增量必须由后续实测证明。

Mac锁屏已在本轮CUA检查确认；新核心原生配对、全应用Cmd+Q和不定进度截图仍待补，使用当前远程分辨率的要求不变。以下810f及更早证据按原哈希范围保留；最新状态以[实时入口](../../evidence/R09-20261017-CURRENT.json)为准。

## 810f候选与查询修复（历史范围）

当前ARM核心SHA256为 `810fbe7d5c54e033142b5f425e56661f6064c208860eb58664ab58f150a498f8`。ARM UI仍为 `8f476d713d92a5d06e9d96db4f20721ce617cf0c13cfcd8ad045efee03f7921b`，x64 UI仍为 `d9d960a31c17505ccc467a658c653744c7e379bd11206eaa3d214350898e510b`。本次只改动核心导入dashboard聚合及其测试；编译器依赖文件证明这两处不属于UI输入。

旧核心3d9…的百万多文件首轮中，独立导入100万条已完成且Token/积分/账本准确，核心RSS峰值39,600,128字节。随后并发回填的最后一次成功查询看到278,272条，后续dashboard返回stats_query_timeout；失败请求的精确提交数未采集，不能用保存快照或上次成功数冒充。旧完整阶段为FAIL，不能把独立资源探针当整体通过。

只读失败快照显示SQLite选择非覆盖来源索引，首读约3810ms；相同SQL使用已有覆盖索引首读约559ms，后续约98ms，全部返回值相同。当前生产查询在无供应商/模型/路由细筛选时显式选择已有覆盖索引，细筛选保留原查询规划。新增回归检查实际查询的3来源×2采集方式EXPLAIN及独立原SQL逐字段等价。查询2秒预算、P95≤500ms门槛、行数预算、数据库结构与依赖均未放宽。详见[构建与诊断](../../evidence/R09-20261018-COVER-QUERY-BUILD.json)。

## 最新需求与实现

历史全量回填是后台任务，统计展示已提交部分；启动和代理不等待完整历史。固定5秒增量独立于页面刷新，常态新增每次≤10秒可查。按用户修订取消百万行/1GiB必须600秒完成的业务门槛，完整语料、最终准确性、持续推进、资源有界和退出续传继续验收。

检查点SQL聚合进度，不额外扫描JSONL计数。原生界面显示剩余文件数、剩余数据量和已入库条数；发现未知时不定进度，暂停和错误如实显示，待人工归属与未读积压分开。真实reader/parser/writer负责去重、冲突撤回、墓碑和断点；Token和整数纳积分独立保存，未知HTTP/速率/费用显示为未知。管理开关、扫描、归属预览/确认/撤销均已接线。

## 810f候选证据范围（历史）

| 项目 | 当前结论 |
|---|---|
| ARM构建 | 540项测试、fmt、Clippy `-D warnings`、release通过；原539个测试名称全部保留。 |
| 生产链路 | [26组新核心门禁](../../evidence/R09-20261018-COVER-QUERY-CORE-GATES.json)全部通过：18项检查点探针、规范字段、157项导入、OS真实写入拒绝、管理边界、调度、退出、65项边界、保留期限、原代理回归等。写入拒绝使用OS EFBIG，不宣称填满物理磁盘。 |
| 超过1000文件 | 1137文件独立探针29项通过：2274条初始事实、3个明确追加身份、每文件EOF、精确Token/积分、3个5秒未变化轮次零正文读取、完整重扫、正常停止及重启。该场景不冒称属于≤1000文件的常态10秒条件。 |
| x64 | [新x64证据](../../evidence/R09-20261018-COVER-QUERY-MACOS-X64.json)：核心540测试/Clippy/release、157项导入、65项边界和1137文件29项全部通过（本机Rosetta）。UI源码/依赖未受影响，复用同d9d9字节的139测试证据；新私有包仍待生成。 |
| 新核心性能 | 需要同810f哈希完整12阶段。先跑此前失败的百万多文件首轮，含20次增量、3×50查询、300秒空闲和严格快照校验；通过后完整矩阵仅复用这一同哈希阶段。旧3d9的6个通过阶段不复用到新核心。 |
| 原生UI | 旧3d9核心与同8f4 UI的12项黄金矩阵和动态回填/暂停/错误恢复、100次普通刷新、7个刷新/可见性场景、归属与分页/输入证据保留。新核心配对尚未原生录制；最后一次CUA被Mac锁屏阻塞，不能把旧后端运行直接升级为新配对通过。 |
| 私有包 | 旧ARM/x64实际DMG内容、签名/代码身份及隔离自动生命周期保留原哈希范围。810f及相应x64新核心私有包待当前文档和验收收口后生成、复验。OS挂载权限受限，归档内容验证不等于安装。 |

原生UI按当前远程环境验收。旧黄金窗口内容为1440×900和1100×700，原始JPEG外框多1像素原生底框；未裁切或缩放。不要求恢复物理分辨率，也不宣称验证其他物理显示环境。

## 剩余验收与边界

1. 同哈希完整12阶段、跨规模RSS增长、全量Token/积分、代理/查询影响、固定增量及空闲的最终聚合；百万目标阶段已通过。之后补百万快照状态轮询及4种完整百万续传。
2. 当前配对原生矩阵、动态进度及当前候选ARM镜像的实际驻留Cmd+Q已完成；最终ARM/x64文档包仍须生成并验证。设置界面退出与整体驻留生命周期分开，实际按键和自动生命周期均保留各自证据。
3. 依据新哈希刷新80AC/47X逐项证据和最终私有包。现有旧哈希自审未自动沿用；不以计数作为交付完成率。Linux/Windows运行、Windows GUI、物理Intel、OS挂载和真实安装均单列外部边界。

旧错误实现产生的不可证明排除状态不批量清空恢复；既有合法墓碑和保留期限约束继续保留。真实源文件、真实数据库和安装应用未用于本轮验证，也未执行真实数据修复。

## 隔离与清理

验证只使用合成语料和专用配置，实际核心PID由kernel sandbox拒绝真实WorkBuddy、Proxy配置和安装应用的数据访问；真实目录只查元数据。所有TMPDIR/TMP/TEMP均在本会话外置盘专用目录，Python启动前设置；复用主工作区target/和gpui/target/，不执行cargo clean。

每轮先停等子进程，再检查确切owned root的进程/句柄并清理，失败和可捕获中断同样执行finally。报告区的必要快照和私有产物保留。约4002个归属不明的旧系统临时项目项保留，绝不按前缀删除。当前清理数量和系统空间见[实时入口](../../evidence/R09-20261017-CURRENT.json)。

上一版当前报告保留于[查询修复前检查点](../../evidence/R09-20261018-IMPLEMENTATION-REPORT-BEFORE-COVER-QUERY.md)，更早初版亦保留原候选范围。人工后续提交本次Codex参与成果时，在消息末尾trailer区保留一次 `Co-authored-by: Codex <codex@openai.com>`，作者与提交者保持真实用户身份；本任务没有自动提交或发布授权。

## 当前127项证据索引

每条链接指向上述逐项复核表，该行包含实际命名测试和运行断言的当前日志哈希。

| 条目 | 当前结论 | 证据 |
|---|---|---|
| AC001 | PASS | [AC001逐项证明](ACCEPTANCE-AUDIT.md#ac001) |
| AC002 | PASS | [AC002逐项证明](ACCEPTANCE-AUDIT.md#ac002) |
| AC003 | PASS | [AC003逐项证明](ACCEPTANCE-AUDIT.md#ac003) |
| AC004 | PASS | [AC004逐项证明](ACCEPTANCE-AUDIT.md#ac004) |
| AC005 | PASS | [AC005逐项证明](ACCEPTANCE-AUDIT.md#ac005) |
| AC006 | PASS | [AC006逐项证明](ACCEPTANCE-AUDIT.md#ac006) |
| AC007 | PASS | [AC007逐项证明](ACCEPTANCE-AUDIT.md#ac007) |
| AC008 | PASS | [AC008逐项证明](ACCEPTANCE-AUDIT.md#ac008) |
| AC009 | PASS | [AC009逐项证明](ACCEPTANCE-AUDIT.md#ac009) |
| AC010 | PASS | [AC010逐项证明](ACCEPTANCE-AUDIT.md#ac010) |
| AC011 | PASS | [AC011逐项证明](ACCEPTANCE-AUDIT.md#ac011) |
| AC012 | PASS | [AC012逐项证明](ACCEPTANCE-AUDIT.md#ac012) |
| AC013 | PASS | [AC013逐项证明](ACCEPTANCE-AUDIT.md#ac013) |
| AC014 | PASS | [AC014逐项证明](ACCEPTANCE-AUDIT.md#ac014) |
| AC015 | PASS | [AC015逐项证明](ACCEPTANCE-AUDIT.md#ac015) |
| AC016 | PASS | [AC016逐项证明](ACCEPTANCE-AUDIT.md#ac016) |
| AC017 | PASS | [AC017逐项证明](ACCEPTANCE-AUDIT.md#ac017) |
| AC018 | PASS | [AC018逐项证明](ACCEPTANCE-AUDIT.md#ac018) |
| AC019 | PASS | [AC019逐项证明](ACCEPTANCE-AUDIT.md#ac019) |
| AC020 | PASS | [AC020逐项证明](ACCEPTANCE-AUDIT.md#ac020) |
| AC021 | PASS | [AC021逐项证明](ACCEPTANCE-AUDIT.md#ac021) |
| AC022 | PASS | [AC022逐项证明](ACCEPTANCE-AUDIT.md#ac022) |
| AC023 | PASS | [AC023逐项证明](ACCEPTANCE-AUDIT.md#ac023) |
| AC024 | PASS | [AC024逐项证明](ACCEPTANCE-AUDIT.md#ac024) |
| AC025 | PASS | [AC025逐项证明](ACCEPTANCE-AUDIT.md#ac025) |
| AC026 | PASS | [AC026逐项证明](ACCEPTANCE-AUDIT.md#ac026) |
| AC027 | PASS | [AC027逐项证明](ACCEPTANCE-AUDIT.md#ac027) |
| AC028 | PASS | [AC028逐项证明](ACCEPTANCE-AUDIT.md#ac028) |
| AC029 | PASS | [AC029逐项证明](ACCEPTANCE-AUDIT.md#ac029) |
| AC030 | PASS | [AC030逐项证明](ACCEPTANCE-AUDIT.md#ac030) |
| AC031 | PASS | [AC031逐项证明](ACCEPTANCE-AUDIT.md#ac031) |
| AC032 | PASS | [AC032逐项证明](ACCEPTANCE-AUDIT.md#ac032) |
| AC033 | PASS | [AC033逐项证明](ACCEPTANCE-AUDIT.md#ac033) |
| AC034 | PASS | [AC034逐项证明](ACCEPTANCE-AUDIT.md#ac034) |
| AC035 | PASS | [AC035逐项证明](ACCEPTANCE-AUDIT.md#ac035) |
| AC036 | PASS | [AC036逐项证明](ACCEPTANCE-AUDIT.md#ac036) |
| AC037 | PASS | [AC037逐项证明](ACCEPTANCE-AUDIT.md#ac037) |
| AC038 | PASS | [AC038逐项证明](ACCEPTANCE-AUDIT.md#ac038) |
| AC039 | PASS | [AC039逐项证明](ACCEPTANCE-AUDIT.md#ac039) |
| AC040 | PASS | [AC040逐项证明](ACCEPTANCE-AUDIT.md#ac040) |
| AC041 | PASS | [AC041逐项证明](ACCEPTANCE-AUDIT.md#ac041) |
| AC042 | PASS | [AC042逐项证明](ACCEPTANCE-AUDIT.md#ac042) |
| AC043 | PASS | [AC043逐项证明](ACCEPTANCE-AUDIT.md#ac043) |
| AC044 | PASS | [AC044逐项证明](ACCEPTANCE-AUDIT.md#ac044) |
| AC045 | PASS | [AC045逐项证明](ACCEPTANCE-AUDIT.md#ac045) |
| AC046 | PASS | [AC046逐项证明](ACCEPTANCE-AUDIT.md#ac046) |
| AC047 | PASS | [AC047逐项证明](ACCEPTANCE-AUDIT.md#ac047) |
| AC048 | PASS | [AC048逐项证明](ACCEPTANCE-AUDIT.md#ac048) |
| AC049 | PASS | [AC049逐项证明](ACCEPTANCE-AUDIT.md#ac049) |
| AC050 | PASS | [AC050逐项证明](ACCEPTANCE-AUDIT.md#ac050) |
| AC051 | PASS | [AC051逐项证明](ACCEPTANCE-AUDIT.md#ac051) |
| AC052 | PASS | [AC052逐项证明](ACCEPTANCE-AUDIT.md#ac052) |
| AC053 | PASS | [AC053逐项证明](ACCEPTANCE-AUDIT.md#ac053) |
| AC054 | PASS | [AC054逐项证明](ACCEPTANCE-AUDIT.md#ac054) |
| AC055 | PASS | [AC055逐项证明](ACCEPTANCE-AUDIT.md#ac055) |
| AC056 | PASS | [AC056逐项证明](ACCEPTANCE-AUDIT.md#ac056) |
| AC057 | PASS | [AC057逐项证明](ACCEPTANCE-AUDIT.md#ac057) |
| AC058 | PASS | [AC058逐项证明](ACCEPTANCE-AUDIT.md#ac058) |
| AC059 | PASS | [AC059逐项证明](ACCEPTANCE-AUDIT.md#ac059) |
| AC060 | PASS | [AC060逐项证明](ACCEPTANCE-AUDIT.md#ac060) |
| AC061 | PASS | [AC061逐项证明](ACCEPTANCE-AUDIT.md#ac061) |
| AC062 | PASS | [AC062逐项证明](ACCEPTANCE-AUDIT.md#ac062) |
| AC063 | PASS | [AC063逐项证明](ACCEPTANCE-AUDIT.md#ac063) |
| AC064 | PASS | [AC064逐项证明](ACCEPTANCE-AUDIT.md#ac064) |
| AC065 | PASS | [AC065逐项证明](ACCEPTANCE-AUDIT.md#ac065) |
| AC066 | PASS | [AC066逐项证明](ACCEPTANCE-AUDIT.md#ac066) |
| AC067 | PASS | [AC067逐项证明](ACCEPTANCE-AUDIT.md#ac067) |
| AC068 | PASS | [AC068逐项证明](ACCEPTANCE-AUDIT.md#ac068) |
| AC069 | PASS | [AC069逐项证明](ACCEPTANCE-AUDIT.md#ac069) |
| AC070 | PASS | [AC070逐项证明](ACCEPTANCE-AUDIT.md#ac070) |
| AC071 | PASS | [AC071逐项证明](ACCEPTANCE-AUDIT.md#ac071) |
| AC072 | PASS | [AC072逐项证明](ACCEPTANCE-AUDIT.md#ac072) |
| AC073 | PASS | [AC073逐项证明](ACCEPTANCE-AUDIT.md#ac073) |
| AC074 | PASS | [AC074逐项证明](ACCEPTANCE-AUDIT.md#ac074) |
| AC075 | PASS | [AC075逐项证明](ACCEPTANCE-AUDIT.md#ac075) |
| AC076 | PASS | [AC076逐项证明](ACCEPTANCE-AUDIT.md#ac076) |
| AC077 | BLOCKED_EXTERNAL | [AC077逐项证明](ACCEPTANCE-AUDIT.md#ac077) |
| AC078 | PASS | [AC078逐项证明](ACCEPTANCE-AUDIT.md#ac078) |
| AC079 | PASS | [AC079逐项证明](ACCEPTANCE-AUDIT.md#ac079) |
| AC080 | PASS | [AC080逐项证明](ACCEPTANCE-AUDIT.md#ac080) |
| X01 | PASS | [X01逐项证明](CROSS-CUTTING-AUDIT.md#x01) |
| X02 | PASS | [X02逐项证明](CROSS-CUTTING-AUDIT.md#x02) |
| X03 | PASS | [X03逐项证明](CROSS-CUTTING-AUDIT.md#x03) |
| X04 | PASS | [X04逐项证明](CROSS-CUTTING-AUDIT.md#x04) |
| X05 | PASS | [X05逐项证明](CROSS-CUTTING-AUDIT.md#x05) |
| X06 | PASS | [X06逐项证明](CROSS-CUTTING-AUDIT.md#x06) |
| X07 | PASS | [X07逐项证明](CROSS-CUTTING-AUDIT.md#x07) |
| X08 | PASS | [X08逐项证明](CROSS-CUTTING-AUDIT.md#x08) |
| X09 | PASS | [X09逐项证明](CROSS-CUTTING-AUDIT.md#x09) |
| X10 | PASS | [X10逐项证明](CROSS-CUTTING-AUDIT.md#x10) |
| X11 | PASS | [X11逐项证明](CROSS-CUTTING-AUDIT.md#x11) |
| X12 | PASS | [X12逐项证明](CROSS-CUTTING-AUDIT.md#x12) |
| X13 | PASS | [X13逐项证明](CROSS-CUTTING-AUDIT.md#x13) |
| X14 | PASS | [X14逐项证明](CROSS-CUTTING-AUDIT.md#x14) |
| X15 | PASS | [X15逐项证明](CROSS-CUTTING-AUDIT.md#x15) |
| X16 | PASS | [X16逐项证明](CROSS-CUTTING-AUDIT.md#x16) |
| X17 | PASS | [X17逐项证明](CROSS-CUTTING-AUDIT.md#x17) |
| X18 | PASS | [X18逐项证明](CROSS-CUTTING-AUDIT.md#x18) |
| X19 | PASS | [X19逐项证明](CROSS-CUTTING-AUDIT.md#x19) |
| X20 | PASS | [X20逐项证明](CROSS-CUTTING-AUDIT.md#x20) |
| X21 | PASS | [X21逐项证明](CROSS-CUTTING-AUDIT.md#x21) |
| X22 | PASS | [X22逐项证明](CROSS-CUTTING-AUDIT.md#x22) |
| X23 | PASS | [X23逐项证明](CROSS-CUTTING-AUDIT.md#x23) |
| X24 | PASS | [X24逐项证明](CROSS-CUTTING-AUDIT.md#x24) |
| X25 | PASS | [X25逐项证明](CROSS-CUTTING-AUDIT.md#x25) |
| X26 | PASS | [X26逐项证明](CROSS-CUTTING-AUDIT.md#x26) |
| X27 | PASS | [X27逐项证明](CROSS-CUTTING-AUDIT.md#x27) |
| X28 | PASS | [X28逐项证明](CROSS-CUTTING-AUDIT.md#x28) |
| X29 | PASS | [X29逐项证明](CROSS-CUTTING-AUDIT.md#x29) |
| X30 | PASS | [X30逐项证明](CROSS-CUTTING-AUDIT.md#x30) |
| X31 | PASS | [X31逐项证明](CROSS-CUTTING-AUDIT.md#x31) |
| X32 | PASS | [X32逐项证明](CROSS-CUTTING-AUDIT.md#x32) |
| X33 | PASS | [X33逐项证明](CROSS-CUTTING-AUDIT.md#x33) |
| X34 | PASS | [X34逐项证明](CROSS-CUTTING-AUDIT.md#x34) |
| X35 | PASS | [X35逐项证明](CROSS-CUTTING-AUDIT.md#x35) |
| X36 | PASS | [X36逐项证明](CROSS-CUTTING-AUDIT.md#x36) |
| X37 | PASS | [X37逐项证明](CROSS-CUTTING-AUDIT.md#x37) |
| X38 | PASS | [X38逐项证明](CROSS-CUTTING-AUDIT.md#x38) |
| X39 | PASS | [X39逐项证明](CROSS-CUTTING-AUDIT.md#x39) |
| X40 | PASS | [X40逐项证明](CROSS-CUTTING-AUDIT.md#x40) |
| X41 | PASS | [X41逐项证明](CROSS-CUTTING-AUDIT.md#x41) |
| X42 | PASS | [X42逐项证明](CROSS-CUTTING-AUDIT.md#x42) |
| X43 | PASS | [X43逐项证明](CROSS-CUTTING-AUDIT.md#x43) |
| X44 | PASS | [X44逐项证明](CROSS-CUTTING-AUDIT.md#x44) |
| X45 | PASS | [X45逐项证明](CROSS-CUTTING-AUDIT.md#x45) |
| X46 | PASS | [X46逐项证明](CROSS-CUTTING-AUDIT.md#x46) |
| X47 | PASS | [X47逐项证明](CROSS-CUTTING-AUDIT.md#x47) |


## 2026-10-19 提交前回归

提交前修正 CI 脚本中遗留的控制协议 11、数据库 schema 7 断言，当前统一验证协议 12 与 schema 8；兼容性用例补充协议 11 的旧核心替换场景。黄金与固定样本验收取消手动来源确认，使用真实 HTTP 关联头证明代理明细优先。原始 JSONL 与旧口径 oracle 保留，新增独立的 `expected-two-model.json`：所有自定义模型自动入库，固定样本共 8 条、838 Token、2.13 积分，其中 1 条重复日志由代理明细覆盖。

补充执行导入主链路（158 项断言）、真实文件系统写入失败与崩溃恢复，以及供应商、协议、统计、启动检查回归，全部通过。Rust/SQL 与已验收的核心及界面构建哈希一致，本轮未重复构建。机器可读证据见 `evidence/COMMIT-GATES-20261019.json`。
