# Studio V2 会话交接与恢复状态（2026-11-01）

此前用户要求暂停并换会话；2026-11-01已按本轮明确请求恢复Goal为active，仍从R01推进。恢复动作已核对Git/远端/独立Goal；禁止自动开启旁白。以下旧轮证据保留原来源，不能混称当前候选。

## 最新进展（QA80）

- 最新运行时代码 `9570451`，Core `13c4998`（SHA256 `1146f58a`）、UI `46585f86`；代码已推送，证据/时间线为其后docs提交，以Git核对最终HEAD。[QA80总结](benchmarks/rc-round80-summary.json)。
- 已修复SQLite启动Busy/Locked导致stats永久停用；可关闭的250ms迁移重试，永久/schema-too-new仍拒绝，recovery/run/interval同事务。三项确定性红绿回归、真实启动前7秒锁释放同PID恢复通过。Core580/27命令/91源码hash，GPUI213pass+2既有ignore/6检查/77源码hash、158导入/15故障/19发现/10创建和真实Core ignored补验全部通过。首个probe文件名不匹配是fixture错误，原件保留。
- 条件列表通知与精确投影缓存已原生验复选/仅选2→1→0、反选/清空、搜索、键盘、主题语言往返及冷重开。长显示名真实截断，但正常单击未观察到完整Tooltip，AX全名不当提示证据。R01全状态/实际slot、真实拖宽、逐引擎IME、真实AT仍缺；**禁止自动开启旁白/系统辅助功能/更改输入源。**
- 最终4096弹窗900秒UI RSS+1.25MiB、CPU6.707%单核，未证明CPU改善；allocated27.3M稳定、碎片+0.8M。一次Tab后300秒CPU1.693%/RSS−.063MiB，但真实焦点目标未确立，不能断言光标根因。保留4905/6d93/4658全部观察和stripped profiler，不再用猜测优化替代诊断。[长时证据](benchmarks/startup-recovery/long-observe/observation-comparison.json)。
- 最终同Core1146基线a6e/UIba525→当前4658的连续主P2/C3/C4/P3已完成；原current1额外菜单操作污染，原三轮诊断全部保留，新顺序在P2开始前预定。UI空闲中位差0、合计−.515MiB、峰值−1.865MiB、启动marker+209.764ms/p95−.0035ms；1562ms慢样本保留，n2描述值不是首帧/FPS/等价结论，R03.2/.3仍pending。[全部7轮](benchmarks/startup-recovery/native-final957/summary.json)。
- 当前957 Core41246370582/Desktop41246381635四job实际success，4份ZIP size/SHA及原JSON/两Windows GDI原PNG已核对。13c/b2各四job成功，7bec Desktop也已全绿但Core156/2首次失败不抹除。一次SSL push失败后误派发旧13c两个自有重复run，核实head后取消，正常push/remote校验才派发正确源码；未TLS绕过/重跑到绿。[最终CI/产物](benchmarks/startup-recovery/ci-957/artifact-validation.json)。
- **纠正QA79原生失败解释：** fixture的B05_TARGET_ID为不合法denied.final前缀，guard要求b05target；CLI预检错误79/正确0，现原生轮正常。历史UI退出码未知，不能断言当时退出79；撤回CUA服务故障归因，原始故障材料保留。
- 原生longfixture1014请求/成本逐字段一致，原配置字段全部保留；四次允许language写只补384个既有typed defaults（64protocol+64settings+256capsnull），**配置字节/原始结构并非不变**。provider/model写0；基准harness的合成save64 setup另计。所有本轮UI/Core/mock/driver已退出，UI由CUA拥有，退出码不可得。
- 精确专用根`studio-v2-startup-recovery.fe_ffldu`已按完整预检/即时复核删除1970文件、1,194,828,019B约1.11GiB，本轮临时保留0。**不要重启已删除bundle。** 必要基线50,905,007B约48.55MiB与共享缓存/非本轮preview保留；系统盘约33.91GiB。[清理](benchmarks/startup-recovery/cleanup.json)。
- **剩余界面0/40，15项发布任务pending，Goal active。** R03.1/R03.4/R04.1已签收。下一步继续R01真实缺口及R03启动慢样本/焦点CPU/长时结论，再推进平台、真实旧版升级、签名公证和分发；没有公开发布授权，不重做40帧或21主题。

## 上一轮进展（QA79）

- 运行时代码已推送：`771a7a7`（读取线程恢复保留已接受的scan_id与状态）、`7bec6cc`（过期/繁忙批次重试成功不再误标失败，永久存储错误仍失败）。当前Core SHA256 `7fa463c3788be3c91b3ec2b929544a23ad24c648fa342cf57d68085a71ad8b34`，UI仍`490530dd`；源码87文件hash/24命令全部本地通过，Core577pass，真实158导入/15故障/19发现/10创建及两既有GPUIignore补验通过。[本轮总结](benchmarks/rc-round79-summary.json)。
- 首次失败均保留，没有放宽期限/ignore/重跑到绿。771的Core41235308376与Desktop内Windows/arm64success，**失败job124637734315实际是macos-15-intel**（依据job.name，不能按ID顺序猜）；同scan_id failed/已提交全字节/无pending、writerfailure1。真实writerACK注入已红绿复现这一失败类，但旧日志没拒绝code，不能断言历史根因唯一确定。
- **最新7bec Core41237875609单元577pass，但导入集成156pass/2fail：7秒DB锁释放后30秒新增行仍4/5。Desktop41237884989仍运行，下一轮只读原run终态/产物，不再dispatch同SHA。** 原始失败报告artifact13056448354/digest已核对，未重跑，R03.4/R04.1继续pending。
- 新独立确定性诊断：启动前持有SQLite EXCLUSIVE锁，代理healthready而stats进入disabled/error_code=stats_storage_unavailable；释放后5秒仍disabled。源码`stats::writer_thread`对Writer::open一次失败即drain_control_without_writer永久停统计，可复现一个待修故障类。原CI未带Corelog/status，尚不能确定它就是远端首次失败原因。[报告/原日志](benchmarks/scan-retry-final/startup-lock-diagnostic/report.json)。**下一步优先修启动锁恢复，保留永久schema拒绝；重试必须避免recover/start_run后半途失败造成重复collection interval或丢失Recovery报告，先审查事务边界/补确定性回归，再跑全套新SHA CI。** 给集成锁失败报告补固定安全的health/status与日志错误码诊断，不能只把ready等待挪后掩盖产品故障。
- 2da原Core41231726063/Desktop41231731650现在**四job全部success**；两Mac原生runner artifact13054881268（arm64）/13054867223（Intel）digest与3份原始JSON各已核对归档。它们是旧Core25f，不能冒充当前7bec全绿。Windows2da两尺寸原图证据保持QA78范围。
- 中间771/f807Core+4905UI与完整旧a6e/25f基线已做原生P/C/C/P：空闲合计差-0.07MiB、UI+0.14MiB、峰值+1.815MiB；全6份原样本/实际1440×900 JPEG/AX/过程/资源保持。另C3/P3发现current3图形驻留125.6M/graphics62.6M在15/30秒降70.8M/10.1M，只解释该短负载观测，不能解释QA78另4096弹窗600秒+2.266MiB。[阶段报告](benchmarks/scan-recovery/performance-stage1-summary.json)。不得将f807阶段重标为最终7faCore。
- 最终7bec完整原生P/C/C/P的首轮后台getApp被CUA `-10005: timeoutReached`阻断，未发现UI/gpui.json、没有ACK或测量，未重试。精确终止自有Core后wrapper/core/mock已等待，Core/mock为-15、driver1，正常故障记录保留；没有称正常退出0。另最终Core独立headless四轮P/C/C/P通过（Core0/mock-15），空闲+0.13MiB、峰值+1.785MiB；候选就绪468.278/52.452ms对基线52.308/51.623ms，慢样本原因未定且全部保留。它不是最终GUI对照/首帧/FPS或精确因果相减。[无界面控制](benchmarks/scan-retry-final/headless-control/summary.json)、[原生失败](benchmarks/scan-retry-final/native-final-attempt/native-cua-failure.json)。
- 本轮所有本地Core/UI/mock/driver已退出；两精确专用根`studio-v2-scan-recovery.7kp2lrar`（1657文件655,827,336B）和`studio-v2-startup-lock.in3wv8el`（11文件284,230B）经全预检、无引用及即时复核后已非强制删除，合计约625.72MiB，临时保留0。**不要重启这些已删除bundle。** 必要基线参考现包含UIba525+Core25f+manifest，50,905,007B（约48.55MiB），共享缓存/非本轮preview保留；清理后系统盘约33.90GiB。[清理](benchmarks/scan-retry-final/cleanup.json)。
- 40帧仍已签收，剩余0/40；18项发布任务pending，Goal active。R01真实全状态/slot、拖宽、逐引擎IME、真实AT缺口不变，不重复能力未变的拖动/长按/21主题。**禁止自动启动旁白/系统辅助功能/切输入源。** 未公开发布。

## 上一轮进展（QA78）

- 已推送运行时代码 `74b1e09`（Windows工具栏top8→32，避开24px caption区域）、CI助手 `2da670b`（仅受限临时CI显示准备/恢复、GDI原图、最终失败门禁）。本轮证据/时间线在其后，读Git获取准确HEAD。Mac UI字节仍`490530dd`、Core仍`25f24ad6`；208pass/2既有ignore、fmt/strictClippy/Intelcheck/release均通过。[总结](benchmarks/rc-round78-summary.json)。
- b167 Core41227146468/Desktop41227149652四job全部success，3平台artifact digest核对后JSON报告归档。74b Core41230163774首次积分升级full-scan15秒失败（571pass/1fail），同SHA一次重跑成功；首次失败及writer_batch_failure1保留，原因未定，R03.4仍pending，不自动重试到绿。74b两Mac success、Windows新增GDI因DPI96工作区容不下1440×900失败，0截图，harness正常退出并清理。
- **当前2da Core41231726063、Desktop内Windows job124627180181已success；Desktop41231731650的两Mac仍运行，下一轮读取原run终态/产物，不重新dispatch。** Windows已枚举并临时由1024×768改1920×1080，96DPI不变，结束后完整恢复校验成功；两尺寸原PNG逐图/hash复核caption顶行与业务工具栏下排无可见重叠，installer和末端强制gate均success，见[原图与审查](benchmarks/windows-native-2da/visual-review.json)。默认英文空请求代表页，不是全Windows画板签收。Windows输入/IME/真实缩放/托盘、旧版本升级与分发行为尚未验。安装器独立执行后仍会检查截图失败，不允许continue-on-error隐藏门禁。
- 同4905/Core25f新隔离64供应商/256模型/1014真实请求/4096候选已实际滚动/搜索/分页末页。发现弹窗600秒RSS中位+2.266MiB、CPU6.656%单核；普通语言菜单另600秒-0.109MiB、CPU1.605%。不同进程/操作历史，不是匹配A/B，不签收无增长或最终性能。三次原生关闭重开，Core同PID；四UI最终均关闭、Core/driver退出0。配置字节及1014请求/成本逐字段保持。
- 本机补fullscreen动作/zoom/角拖动和toolbar恢复；快捷键退全屏、真实minimize/Dock恢复仍未证实。29张本轮CUA原始JPEG及另2张真实Windows GDI原始PNG保留，未转换/替代。R01全状态/实际slot、真实拖宽、当前逐引擎IME与真实AT仍缺；不要重复能力不变的拖动/长按试验。**严禁自动启动旁白、系统辅助功能或切输入源。**
- a6e9618基线已重建`ba525f80`，同manifest/lock，尚无最终P/C/C/P。共享Cargo产物覆盖被hash检查拦截，4905已恢复；后续启动必验hash，不能信任cargo fresh即平面输出正确。保留本地必要参考 `/Volumes/1TB/Downloads/git/workbuddy-proxy/gpui/target/studio-v2-baselines/a6e9618-arm64-release-ba525f80d2a1/`（二进制+manifest约40.5MiB），不是新Cargo target/公开产物，先核验owner/hash再复用。性能原历史成本保留。
- 本轮专用 `studio-v2-release-check.s293t_2n`（518文件约428MiB）及后续 `studio-v2-win-native._bhrl1tr`（2文件约58MiB）均已精确删除，合计约486MiB，临时保留0；不要重新启动里面的bundle。必要原图/报告/审核用脚本已归档，脚本中的SESSION_ROOT是历史占位，不能直接执行。共享缓存和非本轮旧目录未清。清理后系统盘约33.94GiB。
- 下一步：先核验最新Desktop原run；R01只补有能力支持的真实缺口；诊断Ubuntu扫描失败并做最终累计匹配性能，之后继续平台/安装/分发门禁。仍剩0/40界面、18发布任务pending，Goal active，无公开发布授权；不把当前补证当正式版完成。

## 上一轮进展（QA77）

- 代码与证据提交 `5fae93d`；最终候选SHA256 `490530ddf1d8f4aa964dddd86de963924c01fcd9ea12bd807b13a68ae8cc9db5`；fmt、207pass/2既有ignore、strict Clippy、Intel all-targets check、arm64 Release通过。新增具名选择项、模型行完整身份/选择文字、Dialog名称与modal元数据。[总结](benchmarks/r01-states/summary.json)。
- 36wrapper/118限定调用点、10适用状态族只形成明确矩阵，未签收全状态/实际slot。native selected对Row受macOS adapter限制；菜单selected是cursor而非committed checked证明，close为Dialog body兄弟，真实AT仍待。禁止旁白/系统辅助功能/切源。
- 旧首边界drag实际Time列固定；本轮正确provider/model边界两点drag仍无变化。能力未变不重复；刷新/resize保持仍未验证。Tooltip实际文本未截断；长按期间截图被工具串行到释放后，均不标通过。
- 本轮Core配置字节、14请求/成本逐字段保持，写命令0；三UI关闭，Core/驱动0。`studio-v2-r01-states.jfke8w09`58临时文件约253MiB已精确清理，临时保留0，不再启动已删除bundle；共享缓存/旧基准Core保留。
- 下一步仍R01余项集中推进；0/40界面、18发布任务待验，Goal active，未公开发布。旧631599/11da证据不重标最终4905；33张为原始JPEG，控件精确geometry未测。

## 上一轮恢复进展（QA76）

- 本轮代码与证据提交 `0fa0933`；最终候选SHA256 `631599c2bc50295cb8164ed6be583974b7a3d01bfb7568739e0655d25133ccfd`；GPUI207pass/2既有ignore、fmt、strict Clippy、Intel类型检查、arm64release通过。源码绑定见[构建](benchmarks/r01-resume/build-final.json)。
- 已修复并真实补验R01.1外部save64活动表刷新、modal延后、dirty草稿保留/重基准/原生冷重开，以及R01.2末页Next disabled后ShiftTab/Return焦点恢复。14请求/成本逐字段保持。[本轮总结](benchmarks/r01-resume/summary.json)。
- 35wrapper/114调用点只是源码slot索引；R01.1全适用状态/实际尺寸仍待。R01.2拖宽仍无真实成功证据。R01.3本轮n/i只落ASCII，当前输入源未确立，未切源；旧Qingjian成功证据单列。R01.4真实AT未做，严禁自动开启旁白/辅助功能。剩余界面0/40、仍18项发布任务待验。
- 本轮41份CUA原始截图为JPEG，误用PNG后缀已只改扩展名并记录hash。小窗旧end图实际上是页首，正确末行图另存，见native-verification与image-manifest。
- 本轮UI全部关闭，三Core及驱动退出0；专用`studio-v2-r01-resume.y1i2hhhb`已按完整预检精确清理，129测试文件约273MiB，本轮临时保留0。不要再启动已删除bundle；旧preview/共享缓存/旧基准Core未动。
- 下一步仍按R01余项整批推进，不重做40帧/主题组合，不将本轮两处修复当整项R01签收；再按R02→R03→R04。当前无公开发布授权。

## 当前位置（此前交接基线）

- 工作区：`/Volumes/1TB/codex/worktrees/studio-v2-gpui/workbuddy-proxy`；分支 `feat/studio_v2_gpui`。勿误用另一 checkout，先 `git worktree list --porcelain`、`git status`、核对远端。
- 最新设计：Figma `zxDfIQ4lhNcTXrFT7qk8QZ`，页 `51:2`。B01–B05、40帧/80张两尺寸原生主图及R01.5集中签收已完成。**剩余界面0/40；18项发布任务未通过。** 未获公开发布授权。
- 权威清单：[release_plan.json](release_plan.json)；[时间线](progress_timeline.html)；[本轮总结](benchmarks/r01-input-handoff/summary.json)；[QA75](baseline_and_qa.md)。当前R01仍为部分完成，不能把编译、AX树、短负载等同于发行通过。
- 前三次已推送：db17519（40帧集中签收）、30427fe（Issue3品牌白边修复）、cfb5b95（上轮时间线）。本交接与表格语义代码为后续新提交，读取 `git log` 获取准确SHA。

## 当前轮已完成

请求/汇总表补原生角色、完整可访问名称和行列索引；无视觉、事件、数据计算或Core改动。源码hash与e1f构建一致，fmt、206通过/2既有忽略、strict Clippy、Intel类型检查和arm64 Release通过。b29表格AX/6张两尺寸截图、2ce键盘和e1f最终候选分开记录，没有混淆来源。完整清单及原图hash见[manifest](benchmarks/r01-input-handoff/manifest.json)。

真实held Test重复点击仅save/test各1，controlled update各1；菜单/关闭限制如实见记录。七主题×三语言21实际组合完成，7张代表原图已复核。当前Qingjian逐键合成、Space落字、Esc取消；Return落ASCII未误确认。其他字段有粘贴/撤销/重做/非法数字拒绝抽查，不宣称所有引擎真实IME覆盖。

中文/emoji/组合字符模型名经一次保存，由独立Core/UI冷启动逐码点读回；Cold恢复原名后16条历史及成本均保持。两尺寸原图已查看。Core573测试、19真实发现及创建契约通过，但最终Core兼容整项仍待最终RC统一验收。

## 恢复后的顺序

1. 先处理R01余项，整批集中补验、修正、验收，不要从40帧逐项重做或反复微调。R01.1仍缺唯一组件全状态/实际slot；外部Save64活动表/草稿边界已由QA76补验，忙态去重亦有既有证据，避免重复。
2. R01.2“下一页”disabled后的ShiftTab焦点恢复已由QA76修复补验。表头真实拖宽此前两端点CUA拖动无变化，不能把单测冒称物理拖动成功；只用工具支持的真实输入。其余键盘路径已归档，按缺口补。
3. R01.3按不同输入引擎补composition中Enter/Esc和切换/取消。真实Qingjian已经成功，不需要切系统拼音，也不要把CUA typeText当IME。Key只用合成值，安全AX不读出不能编造精确值。
4. R01.4真实辅助技术仍缺。**用户反对自动开启旁白：禁止再次自动启动VoiceOver、系统辅助功能或更改输入源。** 现旁白已关。七主题三语言已完成，勿重复做21轮。
5. 再按R02→R03→R04。真实Windows/Intel硬件、2x屏幕、安装/签名/公证及CI待有相应环境/凭据时验收；当前只有1920×1080的macOS arm64 1x虚拟显示器。Intel cargo check不等于Intel硬件运行。
6. R03重建同条件最终基准/候选匹配性能并保留原始顺序，现混合交互resource-watch不是性能验收。历史B01/B03成本差异不可抹去；长列表、重开、静置、峰值差异及最终完整报告继续按计划。

## Issue、进程及临时文件

- 仓库 `gloryhui/workbuddy-proxy`，本轮读取仅开放[#3](https://github.com/gloryhui/workbuddy-proxy/issues/3)。Mac1x品牌白边已修复并推送30427fe，Issue正文已更新，保持open，HiDPI/Windows补验尚缺。新会话再读当前issue，勿据历史快照关闭。
- 本轮Main/Cold UI已原生关闭，两Core退出0，宿主会话退出0；16条测试历史只有合成隔离数据。Main测试名未在Main恢复，但其临时配置已随本轮清理，不影响真实用户数据。
- 本轮专用目录 `studio-v2-r01.ci7zkz3s` 已完整核实后删除，303测试文件约273MiB，本轮临时保留0。证据已提交，原私有配置/DB/凭据不保留。不要再尝试启动已删除的测试bundle。
- 共享Cargo缓存未清：`/Volumes/1TB/Downloads/git/workbuddy-proxy/gpui/target` 和该仓库 `target`。复用兼容缓存，不每轮clean或新建target。
- 旧固定Core仍保留于 `.../T/studio-v2-round46.wyah2h0f/baseline-core`，SHA `25f24ad66cfee622d8340ea6bd72bb846a8f0eda827599f1b58e1db34b85a7b5`。这是历史参考路径，复用前验证存在/所有者/hash；不要因本轮暂停删除不属于当前会话的目录。
- 其他旧preview/Core可能在运行，不能根据进程名结束。已归档historical-harness只作审计，含SESSION_ROOT占位，不是可直接执行的通用脚本；新轮需重新创建专用目录、唯一bundle、只允许隔离目标的保护及读写边界。

## 持续协作约束

- 每轮报告剩余界面数量，发布任务单列。完成较大功能后才更新时间线。批次验收后提交推送当前分支，不强推、不改已有历史，不公开发布。
- Computer Use按AGENTS委派专用Terra代理，单一界面同一时间只有一位操作者；优先连接器。不要主动activate/raise。CUA在关闭后立即getAX可能自动重启应用，关闭后用只读PID/元数据确认即可，不应重开。
- Mac锁定时推进离线工作，不解锁、不改权限；不得外部AX/CGEvent/AppleScript绕过工具限制。用户已授权明确归属安全测试产物清理，仍先检查精确绝对路径、所有权、链接、引用，删除使用具体路径非强制形式。
- 提交保留用户Author/Committer，唯一trailer：`Co-authored-by: Codex <codex@openai.com>`。

## 新会话可直接发送

> 继续 Studio V2 Goal。先读 docs/studio_v2/SESSION_HANDOFF.md、release_plan.json 和 AGENTS.md，核对当前分支与远端。从 R01 剩余缺口整批推进，不重做已验40帧。禁止自动开启旁白或系统辅助功能。每轮汇报剩余界面数量，整批通过后提交推送并更新时间线；公开发布另行授权。
