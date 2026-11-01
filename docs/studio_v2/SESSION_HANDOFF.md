# Studio V2 会话交接与恢复状态（2026-11-01）

此前用户要求暂停并换会话；2026-11-01已按本轮明确请求恢复Goal为active，仍从R01推进。恢复动作已核对Git/远端/独立Goal；禁止自动开启旁白。以下旧轮证据保留原来源，不能混称当前候选。

## 最新进展（QA85）

- 用户要求精简测试，已落实`desktop.yml`手动`ui_only=true`入口与独立三平台GPUI reusable workflow，提交`3e257fd`：格式/Clippy/单元/Release/版本CLI，跳过Core、安装器、旧版迁移；默认完整流程保留，README说明范围。只派发一次run`41262088104`绑定3e257fd，实际Windows/ARM/Intel三UI job首轮全部success、完整Desktop/旧升级skipped；[原run/steps/产物元数据及原日志](benchmarks/render-boundary/ci/validation.json)已归档，UI artifact仅核对服务端元数据，未下载/独立哈希，不是分发包。**这不是新的全Core/RC矩阵，运行时仍957/UI4658/Core1146，14项发布待验不变。**
- 宽范围`WindowContent`缓存试验82f已撤回，当前产品源与957一致、共享平面release恢复并核验4658。依赖`gpui-pre0.3.9`锁定原档检查：a11y每帧清空nodes，cached reuse_prepaint/reuse_paint不回放a11y节点/动作；试验中有短暂AX空树/失效ID，不能唯一归因但不交付这个风险。候选补丁/构建/原图全部保留，不能把82f原图当当前957新验收。
- 首P1在用户插入精简问题后开始，CPU2.089%单核；未持续记录active/frontmost，后续Cocoa点测inactive只代表该时点，不反推历史，不据低值算优化。用户反馈前后协议原件均保留；C1/C2/P2未采，未来observer每5秒记录活动状态并在丢失条件时停，不自动重试。因试验已撤回，本轮不再需要前台时段，不主动activate/raise。
- 本机试验fmt/213pass+2既有ignore/strict Clippy/Intelcheck/ARMrelease通过，两个ignore另用真实Core补19发现/10创建通过。候选一轮原生输入撤销/筛选/选择、弹窗开着自动1014→1015、resize/主题/语言/切页通过；误点一次**受控**更新提示、两次失效AX索引均原样记录，无安装/下载。新图原18张尺寸1440×900/901及1100×701，未裁剪/转换。
- 原1014请求/成本与新增第1015行在退出后全字段保持；语言en→zh有意写入使配置补64protocol/64完整typedsettings/256capsnull共384字段，所有原字段值保持，**配置字节不是不变**。初验证器把settings误期望null已按既有`ProtocolSettings::default`精确修正，不重跑产品。Core/fixture0，两个CUA UI只证PIDgone、退出码未知。
- 本轮专用`studio-v2-render-boundary.9r95x1iq`经完整/即时预检精确清理66文件/36目录、320,431,122B约305.59MiB，专用临时0；必要基线50,905,007B/共享缓存保留，系统盘约35.67GiB。上一轮边界外Chrome socket约20B仍未动。**不要重开已删除bundle；禁止自动开启旁白/辅助功能/切源。** R01/R02/R03实际缺口仍在，下一步需A11y安全的渲染方案，别再次直接缓存整个Proxy。

## 上一轮进展（QA84）

- R03焦点缺口已有实际证据：同UI18545/Core18142、64供应商/256模型/1014请求与成本/4096候选，预定A1/B1/A2/B2各90秒。A实际z→匹配0→Backspace恢复共同前缀/4096/0；B实际Tab×2、Space反选0→4096→0，查询与匹配不变。CPU依序6.455/1.522/6.355/1.478%单核，均值搜索6.405、非文本1.500。具体AX焦点仍只window，按钮身份据顺序/视觉/行为推断，但非文本键盘路由已证，不再只是Tab关联。[QA84](benchmarks/rc-round84-summary.json)、[原样本/对照](benchmarks/focus-profile/comparison.json)。
- **运行时代码仍957/UI4658/Core1146，没有性能修复。** 仅cargo rustc覆盖strip生成符号副本，23个文件内容节字节和VM地址全部相同；实际wrapper仅签名变化已证明。4份真实sample经atos/锁定rustc-demangle解析：A含BlinkCursor、Proxy render、Taffy布局，B均未采到。原始sample保留，不把递归采样次数相加当CPU归因；定位焦点相关重绘路径，尚非唯一逐周期根因。共享平面release已恢复并核验原4658，依赖源与锁定crate原档一致。[符号证据](benchmarks/focus-profile/symbol-identity.json)。
- 下一步针对未变子树验证有明确失效通知的缓存/渲染边界，保留500ms光标、正常输入/IME、选择和4096负载；不能关光标/减负载/降门槛过关。GPUI mark_view_dirty会标记祖先，简单缓存整个root仍被输入失效传播；先验证边界，再做最终性能回归。启动慢marker和长期驻留结论仍缺，**R03.2/.3/.5不签收**，R01状态/slot/拖列/逐引擎IME/真实AT仍保留。
- 10张原始JPEG均1440×900及各AX已保留，最终Cancel/原生close，没有添加/保存；退出后配置字节与1014请求/成本全字段相同。Core0/driver0，CUA拥有UI只证PIDgone、退出码未知。A1初sampler预检枚举名不匹配在计时前停下、两个诊断helper编译配置错误均记录，实际4轮协议不变；无产品失败重跑。[数据/退出](benchmarks/focus-profile/data-proof.json)。
- 专用根`studio-v2-focus-profile.fmt6q01u`经完整及即时预检，精确非强制删除50文件/20目录、157,128,912B约149.85MiB，临时0、系统盘约35.78GiB；必要基线50,905,007B/共享Cargo/非本轮preview保留。**不要重开已删除bundle；禁止自动开启旁白、辅助功能或切输入源。**[清理](benchmarks/focus-profile/cleanup.json)。
- 用户询问时间线卡片：5/4/5/5是各批次总任务数，实际待验R01=4、R02=4、R03=3、R04=3，共14；40帧均已签收。HTML已于08:51:49统一生成QA84，卡片增加实际通过/待验数。IAB不可用原记录保留；已有Chrome独立headless/私有profile两尺寸1100/1440渲染及DOM核对通过，原图已查看，无横向溢出；首组smooth scroll未到目标的原图保留，stable仅用即时滚动重采。[时间线核对](benchmarks/timeline-qa84/parent-review.json)。Goal active，无新增整项签收/CI或公开发布。
- 时间线预览server已退出0、独立headless进程均结束；额外专用profile经完整及即时预检已精确清理9,341,455B约8.91MiB，连同主fixture本轮专用产物逻辑约158.76MiB均清理。其外部Chrome临时socket目录`com.google.Chrome.16SEjC`约20B因归属/边界未扩展删除而保留，不能说系统临时文件绝对0；两专用根均不存在。必要基线/共享缓存照旧保留。[浏览器清理](benchmarks/timeline-qa84/profile-cleanup.json)。

## 上一轮进展（QA83）

- 新增真实包冷回滚：test `70712b8`，runtime 仍 `9570451`；Windows/ARM/Intel × v0.5.6/v0.5.7 共 6 例升级后恢复原程序与完整旧 profile，首次 CI `41256459894` 三 job 全部 success。旧 schema6/11、配置/Key 字节及原12有价记录保持，旧 Core 恢复后可新录有价行；候选第13条只在独立保留目录中，保留目录与升级前备份均逐文件字节未变。[QA83](benchmarks/rc-round83-summary.json)、[六例核对](benchmarks/cold-rollback/ci/validation.json)。
- Windows 使用候选 uninstaller 后原 setup 同路径重装，原244/16文件SHA、版本、注册/快捷方式一致；两Mac保留候选app、恢复原app并核对全树/签名。候选已装GUI生命周期通过，回滚后只验旧Core，不冒称旧GUI/物理输入/热降级/历史自动合并/WorkBuddy会话回退。直接CLI每例3个Core全部0/waited，nativehost0、其子进程仅PIDgone证明。原Desktop全构建故意skipped，不称新的Core全矩阵。
- 本机最初两次在旧Core退出0后、替换前的cold readonly查询失败，报告原样保留。发现SQLite上下文未显式close并修复，确定性回归已验；closing单独未解决故障。独立冷WAL副本复现RO CANTOPEN而immutable可读；现仅已停/无活跃元数据且WAL空或不存在时immutable，运行中仍普通RO，非空WAL/活跃marker明确拒绝。最终本机ARM两例与CI六例通过，不归因产品迁移错误。[诊断](benchmarks/cold-rollback/readonly-wal-diagnostic.json)。
- [备份与冷回滚步骤](UPGRADE_ROLLBACK.md)已加入README链接，原包URL/SHA固定；正式版本、changelog、公开截图与最终分发包仍缺，**R04.4继续pending，全部发布待验14项；剩余界面0/40，Goal active。** 其余R01/R02/R03缺口不因回滚通过而豁免。
- 全部本机进程/挂载结束；精确清理本轮`studio-v2-cold-rollback.5eng36js`694文件/301子目录、607,038,996B约578.92MiB，专用临时0，系统盘约33.86GiB。三runner各清理0，合计815,012,981B不计入本机释放。必要基线50,905,007B/共享Cargo/非本轮preview保留。**不要重开已删除bundle；禁止自动开启旁白、系统辅助功能或切输入源。**[清理](benchmarks/cold-rollback/cleanup.json)。

## 上一轮进展（QA82）

- **R04.2真实安装/覆盖升级/卸载已签收，发布pending15→14。** 实际Windows旧setup→候选setup同一Unicode自选目录覆盖、Mac真实DMG app同路径整体替换（旧app留自有备份）；旧版在安装前已写12有价行、价格、731天/OBSERVE与Unicode配置。v056/schema6与v057/schema11×Win/ARM/Intel 6例首轮通过，安装本身保持旧数据、当前schema11新增第13有价行、已装GUI生命周期后全13字段及配置Key字节仍同，卸载保持外部profile。[QA82总结](benchmarks/rc-round82-summary.json)、[逐项验收](benchmarks/installed-upgrade/acceptance-audit.json)。
- 测试代码98f2212/0a355e8，运行时代码仍957。冻结三平台包来自原CI41246381635，未编译/修改Core/GPUI/packaging或正式版本。已有desktop手动upgrade_only→同提交reusable矩阵，run41254168866绑定0a355；Intel124693699432/ARM124693699544/Windows124693699610全部success，常规desktop全构建故意skipped，不能称新全Core矩阵。[原始报告与校验](benchmarks/installed-upgrade/ci/validation.json)。
- 已装host0，关闭设置留同Core、重开新UI、Core/UI OS PIDgone、3统计run normal；直接CLI Core0/waited。host拥有的Core/UI不能取得exitcode，不虚报全进程0。Windowsportable所有文件SHA/版本x64/注册/主入口快捷方式、Mac树资源/链接/strict签名/CPU版本一致。CI流程无键鼠/新截图/IME/物理设备验证；冷升级和隔离自选外部profile，不当热更新、默认profile迁移或Gatekeeper/SmartScreen信任验收。
- 三结果ZIP digest及原JSON/log字节保持；Windows脚本仅LF→CRLF，双源SHA派生精确核对。首次本地归档验证器按LF比Windows而停，修正为平台真实换行后重读原ZIP，未重跑测试。源码/运行时/报告来源已区分，原生值截图仍QA81，不重标为本轮图。
- 三remotefixture均清理0，逻辑合计711,111,221B在各runner；本机`studio-v2-installed-upgrade.2pamppmx`经完整/即时预检已精确删除705文件/623,689,644B约594.80MiB，专用临时0、系统盘约33.95GiB。共享缓存与约48.55MiB必要基线/非本轮preview保留。**不要重开已删除bundle。**[本机清理](benchmarks/installed-upgrade/cleanup.json)。
- **剩余界面0/40，14项发布pending，Goal active。禁止自动开启旁白、系统辅助功能或切输入源。** R01真实全状态slot/列drag/逐引擎IME/真实AT、R02物理系统输入/缩放/平台场景、R03marker慢样本/焦点CPU/长驻留结论、R04.3信任签名公证/4版本说明回滚包/5出厂发布仍缺。没有公开发布授权；下一步按实际可用环境继续，不重复能力未变的hover/pressed/headerdrag或重做40帧/21主题。

## 上一轮进展（QA81）

- 新增真实发布版数据验证脚本 `scripts/release_upgrade_test.py`，只新增测试/证据，运行时代码仍957。官方ARM v056/Coreee82/schema6与v057/Core085e/schema11→当前957CI/Core d150，两例实际PASS：旧12请求/成本/手工价/731天/OBSERVE/Unicode配置逐字段与字节保持，当前可新录第13条；所有Core0/等待。原版DMG与当前CI ZIP digest/size、只读挂载复制/签名/detach已核验。[QA81](benchmarks/rc-round81-summary.json)。
- 原生UI来自957 CI原helper63d64ecd，以唯一wrapper隔离重签为2e6a70db（23个文件section保持）。宽1440×901/小1100×701九张原始JPEG/AX已逐图查看：价格USD2/8、当前731天（预设未误选）、OBSERVE、Unicode、13总数/末页3条/可见USD.0028可读，小窗需滚动。不是整页13行逐条视觉校验、精确slot或默认主题/正式签名证明。
- 原有12对照真实released-backup，在迁移与宽/小两native后逐旧字段仍一致；小窗前后全13字段、配置字节一致。所有UI/Core/driver已退出；UI由CUA持有，退出码未知。[数据证明](benchmarks/release-upgrade/native-small-readback-data-proof.json)。
- 三类自有fixture错误均保留并纠正：首快照引用废弃价格表（未启候选）；新Info未重签导致wrapperinvalidInfo/-54（原包valid，未改系统权限）；宽窗收尾读已被UI正常删除的gpui.json（driver1/Core0，不标parentPASS）。后者由livePID捕获与前快照修正，小窗driver0/完整数据证明。未把它们称产品迁移/服务故障。
- R01只读能力复核无新hover/dragpath/pressed/geometry方法，不重复旧试验。**禁止自动开启旁白、系统辅助功能或切输入源。** 全状态/slot、真实列拖宽、当前逐引擎IME、真实AT仍缺；QA80 R03慢marker/CPU/长驻留结论不变。
- **R04.2仍pending。** 目前仅macOS ARM真实Core冷数据交接，未做OS installer覆盖/同目录app替换、热升级/回滚或Windows/Intel验证。下一步在旧数据已存在时运行实际旧setup→候选setup或app同目录替换；为当前脚本分离seed/verify或加明确安装交接钩子，再补真实runner。不要把原synthetic sameversion Core stub测试或本轮冷交接替代此项。
- 本轮`studio-v2-release-upgrade.1ldx41h9`经完整预检/即时复核已精确清理389文件、268,242,426B约255.82MiB，临时0，系统盘约34.00GiB；共享缓存/约48.55MiB基线/非本轮preview保留。**不要重开已删除bundle。** 新fixture若改原签名helper Info，CUA前必须strict签名预检；若需ad-hoc重签记录原/新SHA及代码section一致，不能称正式分发签名。[清理](benchmarks/release-upgrade/cleanup.json)。
- **剩余界面0/40，15项发布任务pending，Goal active。** 没有新增门禁整项签收、没有新全矩阵CI或公开发布；QA80已通过的957运行时代码CI仍按原来源保留。

## 上一轮进展（QA80）

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
