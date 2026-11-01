# Studio V2 会话交接与恢复状态（2026-11-01）

此前用户要求暂停并换会话；2026-11-01按明确请求恢复Goal并从R01推进。QA91软件RC批次完成后，同一外部环境阻塞连续三个Goal回合未变，其后用户提供当前Mac人工操作条件并恢复Goal；QA92已完成Mac人工补验、全屏背景修复及三平台UI/WindowsGDI检查，已清理；其后Goal因人工条件未答标为blocked；用户现已明确跳过当前Mac第二输入法交叉验证，范围已更新，详见下节。禁止自动开启旁白；各旧轮证据保留原来源。

## 当前状态（QA92完成后，用户已跳过第二输入法）

- 用户2026-11-01已明确答复“不用测试另外一种输入法了，跳过”。**当前Mac另一种中文输入法交叉验证已取消，不再等待该可用条件、不再要求用户切换输入法，也不得在后续恢复时重新加入。** 这是明确范围决定，不是另一输入法测试通过；已有青简模型名称Enter/Esc/取消结果保留。结构记录见release_plan.json的R01.3 waived_requirements。
- R01.3仍pending，仅因为尚缺字段的真实composition/输入取消等原范围未完整覆盖；不得把用户只跳过第二输入法扩大成取消全部IME、全部字段或跨平台原生输入。其他R01/R02/R03/R04门禁不变，15发布待验、40帧签收/剩0。本次范围同步不新增QA编号或通过数量。
- 独立Goal面板实际仍为`blocked`，本次未冒称恢复active；此前等待第二输入法答复的阻塞理由已解除，后续继续时从仍缺字段和其余真实环境条件推进，不能再为第二输入法停工。QA92所有原run终态成功、本机测试会话已结束清理，没有待重启或等待的活进程。
- 不启动新测试或修改系统。保持禁止自动旁白/AT/输入源/系统缩放，未经另行授权不公开发布。不重开已清理QA92/86 bundle，不重复已确认的Mac全屏修复、40帧/21主题或后台90/900秒；当前无测试临时文件，RC包/必要baseline和共享缓存保留。

## 最近完成进展（QA92，人工Mac验收与全屏背景修复）

- **当前UI运行源码`85540e1256842a5e6553bffd9749088af545084e`，版本0.6.0，input Mac UI SHA`f756a70901da78e19185b9b9c1e4a29b1cfaaa0ee8fce35b0f8f2cd7772c491c`；Core仍dd9/b920，未改。** 本轮只改ui.rs顶栏背景，将色块移出Kit全屏内层pl_3的布局留白；原生TitleBar透明叠上保留控件/拖动。Kit源逐字节等于锁定原.crate，依赖/registry/schema/输入代码未改。[范围](benchmarks/manual-mac-rc060/delivered-scope.json)。
- 用户已明确当前Mac人工拖供应商列、端口56290刷新缩窗保持、供应商cell悬停完整名称可读；青简显示名称真实候选Enter只落字/弹窗保持、Esc取消和底部取消正常；黄色最小化/Dock恢复及全屏往返正常。人工报告与原图独立分列：after-human-drag1440×901边界仍似初图约574，after-refresh-resize1273×810证缩窗/布局重排，不能独立量化拖宽；候选/Tooltip原帧未捕捉，不扩展所有字段/引擎/AT。[人工原文](benchmarks/manual-mac-rc060/human-observations.json)、[原生记录](benchmarks/manual-mac-rc060/native-final.json)。
- 用户报告全屏顶部浅条并贴270×119原PNG；CUA真全屏1920×1080直接复现约10px浅条/深色外伸。旧工具一次CtrlCmdF未确认退出按失败保留，用户实际退出另行确认。修复普通1440×901/全屏1920×1080/退出后1440×901三原JPEG均对齐，用户最终确认“全屏和退出后都已对齐，浅色条消失”。首fixed windowed Core状态未知原图保留、后图运行中，不抹掉暂态。[红绿](benchmarks/manual-mac-rc060/fullscreen-red-green-review.json)、[11原JPEG及1用户PNG](benchmarks/manual-mac-rc060/image-manifest-final.json)。
- 本机fmt/ARMRelease/CLI0.6通过，签名runtime6dfb190b对inputf756全部非签名字节一致，mutation flag仅比较器自测。**唯一UI-only run41288999354/source855/attempt1正式success，ARM124796131735/Win124796131859/Intel124796131982首轮均成功，watch71333终态0已关闭。** Win215/两Mac216pass，各2既有ignore；fmt/strictClippy/Release/CLI及原日志/三UIartifact元数据核对，未下载三UIZIP本机，不当完整RC或真实硬件验收。[UI CI](benchmarks/manual-mac-rc060/ci/ui/validation.json)。
- 测试helper`f9d3f58`pin新WinUI artifact13076059258/run41288999354与旧dd9Core artifact13072713023。首次pushTLS失败后核远端仍855，再普通push成功；未绕过TLS/force。**唯一native-only run41290170711/sourcef9d/attempt1成功，watch1790终态0已关闭。** WinUI SHA`215b15d6ad1ac91e1c01d2d5bcc78ddb43e0ffa71396515819b8d90ee7b51557`、Core`ed1d6440`，冻结guard实际独立下载source/run/bytes/SHA通过；artifact13077375494/155454B/SHA c31d167e独立下载核验。两原GDI PNG1442×901/1102×701逐图/hash审查caption/toolbar分离与侧栏对齐、小窗筛选换行；96DPI，默认英文1供应商64模型0请求，不签物理输入/IME/HiDPI/全RC。两个runnerfixture205,997,568B清理为0，显示恢复1024×768验证成功。[Win CI/原图](benchmarks/manual-mac-rc060/ci/windows-native/validation.json)。
- 原Mac手动测试UI59772/dd9inputf5fa，修复UI78318/inputf756，均已原生关闭PIDgone；退出码不可得。Core59297及driver59295/句柄61381正常退出0。冷读config字节、整份合成models.json字节、12请求成本/5业务表全字段与基准相同；1normalrun/12admitted/0drop/0writefailure。3076个UI命令均真实Core只读，save/install/set_port0、native只有guard_installed无open/terminate；Corekernel保护已验，GUI只隔离环境+deny-onlyguard，不称GUIkernel。[数据](benchmarks/manual-mac-rc060/data-proof.json)。
- 自有根`/private/var/folders/zg/0qx2fqyj0h5fmsk421t3ypy80000gn/T/studio-v2-manual92-hil6m42y`经完整/即时owner/父链/无symlink/mount/库存hash/lsof0/ps0预检，精确非强制清理45文件/23子目录97,003,904B约92.51MiB；本机专用临时0，必要两baseline97.14MiB、QA91原四包134.37MiB及共享Cargo保留，系统盘约35.68GiB。**禁止重开已删QA92/86 bundle，不再需要用户前台或退出全屏。**[清理](benchmarks/manual-mac-rc060/cleanup.json)。
- **40帧签收/剩0，发布待验15=R01 4/R02 4/R03 3/R04 4，Goal active，未公开发布。** 新UI使R04.1最终全矩阵重新pending；本轮阶段UI/native pass不能重标QA91 dd9完整RC/安装升级包。最终包仍需冻结后绑定验收。下一步只剩实际状态/focus/其他字段引擎/真实AT，真实Win/Intel/多屏/缩放/系统登录/WorkBuddy会话，以及有效前台/启动/峰值/最终性能与分发；不得重复QA92已确认Mac路径、40帧/21主题/后台90/900秒或能力未变尝试。用户禁止自动旁白/AT/切源/系统缩放。QA92证据提交后显式--task92一次更新时间线。[QA92](benchmarks/rc-round92-summary.json)。

## 历史阻塞（QA91后，已由用户恢复）

- 独立Goal工具已实际返回`blocked`；不是complete，也不是用户请求pause。连续三个无进展回合核实同一条件：剩余真实输入/设备/有效前台环境不可由当前工具建立，新的可用环境询问尚无答复。原QA86前置窗口回复属于已结束试验，不作为新窗口就绪授权。
- 第一次只读CUA能力复核无变化：没有native hover/mouseMove、分步down/up或带路径/时长drag；清单不提供真实frontmost/显示器拓扑/缩放，未启动隔离App。后两次Git/清单核对仍为远端相同472a824、干净工作区、14待验；已完成CI没有待继续等待的原run。不重复失败尝试、构建、CI或后台样本。
- 仍40帧签收/剩0，14发布待验=R01 4/R02 4/R03 3/R04 3；QA91完整CI与六例升级回滚签收不撤销。时间线只同步Goal状态，历史任务仍91，不新增QA或验收数量。
- 恢复条件：用户说明可用的人工Mac、Windows/Intel真实设备及相应显示/前台条件，再继续对应剩余门禁。输入源/辅助技术由用户控制；严禁自动旁白、AT、切输入源或改系统缩放。未请求提前公开发布。
- 本次状态同步无新测试进程或临时文件；RC交付包、必要baseline和共享缓存保留。不要重开任何已清理的QA86/QA91测试目录。

## 最近完成进展（QA91，0.6.0 RC）

- **运行／打包冻结源`dd9debade83e91b0d11c6b28f18f97edb227abbd`，版本0.6.0候选，未公开tag/Release。** 按此前已说明的默认0.6准备；业务／渲染`src`/`gpui/src`相对b617/13c未变，依赖图未更新。版本统一、打包标签拒绝漂移、回滚指南随包、Fast许可说明和CHANGELOG已准备；7版本+5路径测试、两metadata、两Release/CLI通过，没有重复本机全套。[候选审查](RC_CANDIDATE.md)。
- 本机ARM Core SHA `b920f91e2c85b7cd40f5f779ab8a7b06e4151ce6779c25444126c270300de2ff`、UI `f5fa721fa5f4d314badbdcc70621e9e91ae24feb1532681449f8eb5138139db5`，最终复核仍同。**唯一Core41278921019与Desktop41278929532，source均dd9/attempt1，四job正式success，watch均0并关闭。** Linux579/桌面各580Core，Win215/两Mac216UI且各2既有ignore，完整集成/安装器/生命周期/GDI通过。Core158导入/15故障、三Desktop各158导入、4ZIP来源/大小/SHA及原报告独立核验；R04.1已签收。[完整CI](benchmarks/rc-060/ci/desktop/validation.json)。
- 4个实际候选包（Win ZIP/setup、ARM/Intel DMG）和各校验文件保留于忽略的`dist/studio-v2-rc/0.6.0`，共140,896,314B约134.37MiB；不是临时垃圾，不删除或提交大二进制。原始Win PNG实际1442×901/1102×701、96DPI，父代理逐图核对0.6footer/caption分离/合成数据；未替换README图片或签物理输入/IME/HiDPI。[包清单](benchmarks/rc-060/candidate-packages.json)、[两原图](benchmarks/rc-060/windows-visual-review.json)。
- 测试/manifest提交`21dea55`绑定新包，原升级run41281534447：Win/ARM4例PASS，Intel v056在candidateCore waited0后、GUI之前被非空WAL冷读断言拒绝；Intelv057未运行。原metadata/独立ZIP/失败日志和报告保留。初本地日志/metadata GET EOF后用GitHub连接器补读，没有重派发原source。[首失败](benchmarks/rc-060/ci/upgrades-first/validation.json)。
- 测试源`c435b5c`：确定性用例证明PASSIVE已全部checkpoint仍可有非空WAL；另证明main-only immutable会遗漏WAL-only提交。红3/2fail、绿3pass；仅将cold非空WAL的读取改为一次性DB+WAL副本上的query-onlySQLite，原文件字节不变，活动标记/13行/全部字段/正常退出/全目录回滚断言保持。没有在原DBcheckpoint/截断，也未改产品。原WAL随原runner安全清理，不能确定当时frames状态或唯一残留原因，不宣称业务丢数。[诊断](benchmarks/rc-060/cold-snapshot-diagnosis.json)。
- **唯一新版升级run41282248853/sourcec435/attempt1正式success，三平台×v056/v057六例真实冷升级/原包+完整profile回滚通过，R04.2恢复签收。** 各平台3WAL回归也过；产物/包大小SHA及脚本LF/精确WinCRLF身份独立核对。只跑upgrade_only，完整Desktop/UI/GDI故意skipped；运行产物始终dd9。watch90774因本地API EOF退出1，不是CI失败；已由原run metadata与连接器job终态补证。所有本轮工具进程已终态，无需再watch或派发这几run。[六例](benchmarks/rc-060/ci/upgrades-final/validation.json)。
- 本机专用`/private/var/folders/zg/0qx2fqyj0h5fmsk421t3ypy80000gn/T/studio-v2-rc060-iwcpielt`经完整/即时预检精确非强制删除4文件140,332,817B；另8个自有回归/快照根582,556B均确认移除，本机专用临时0，必要baseline97.14MiB与共享Cargo缓存保留，系统盘约35.69GiB。首升级3runner fixture及最终3runner fixture/15额外快照回归根全部移除，原报告留在仓库证据内。**禁止重开已删QA86/QA91目录，当前不需要用户前置旧窗口。**[清理](benchmarks/rc-060/cleanup.json)。
- **40帧签收/剩0；发布待验14=R01 4/R02 4/R03 3/R04 3，Goal active。** 不自动开旁白/AT/切源/改系统缩放；未公开发布。下一步先核对人工环境可用答复，只补真实R01/R02及有效前台/启动/峰值性能缺口；不重复40帧、21主题、后台90/900秒或能力未变的hover/pressed/drag。最终公开代表图/长期分发/下载信任与全门禁及另行发布授权仍待。QA91证据提交后显式--task91统一生成时间线一次。[QA91](benchmarks/rc-round91-summary.json)。

## 上一轮进展（QA90）

- 当前运行时仍b617/UI3c0e/Core1146，无新产品代码/构建/CI/性能轮次。本轮实测关闭R01.1此前明确的StartupSync/pendingPort两个子缺口，不再列“未触发”。隔离HOME种1managed旧地址+1unrelated合成条目，实际Core检查expected127.0.0.1:61270/1of1 mismatch/192.0.2.1:61271，非响应注入、未install；1真实HTTP与成本后冻结基准。[前置](benchmarks/modal-remaining/native-preflight.json)。
- 两确认各有1440×901与1100×701原JPEG/AX，父代理四图查看/hash核对，地址/计数/警告/按钮可读。中性Return后两提示均保持；StartupCancel无壳/遮罩，pendingPort从61270拟改61393仅进入确认后Escape，无壳且重开编辑器仍61270；然后正常close。编辑器“应用端口”只调用prepare_port，不是业务set_port确认。未点两个restart按钮。AX仅window，非精确子焦点/完整键盘/IME/AT签收。[原生](benchmarks/modal-remaining/native-result.json)、[四原图](benchmarks/modal-remaining/image-manifest.json)。
- UI只读allowlist，install/set_port/native restart动作0。配置字节、整个合成WorkBuddy models.json（含unrelated）字节、1请求/成本全字段退出后保持，stats normal/1admitted/0drop/0writefailure。Core/driver0，UI仅PIDgone、退出码未知。签名只有signature差异；mutation flag是比较器自测；Corekernel已验，GUI不虚称kernel沙盒。[数据](benchmarks/modal-remaining/data-proof.json)。
- 当前HostARM Mac16,10、Rosetta probe translated1只是可用性，不当Intel硬件/GUI验收。只读DeveloperIDApplication计数0；现有README已经选择Mac ad-hoc未公证/Winunsigned，计数0不单独构成该方案mandatory blocker，真实下载Gatekeeper/SmartScreen仍待。禁止自动开旁白/辅助功能/切输入源/改系统缩放。[环境/待验审计](benchmarks/modal-remaining/remaining-gate-audit.json)。
- 所有本轮进程结束，自有gqbmccz6经完整/即时预检精确清理47文件/19目录、52,824,674B约50.38MiB，专用临时0；必要两基线97.14MiB/共享缓存保留，系统盘约35.67GiB。**禁止重开已删bundle。**[清理](benchmarks/modal-remaining/cleanup.json)。
- **40帧剩0，发布待验15=R01 4/R02 4/R03 3/R04 4，Goal active，未公开发布。** QA90证据提交后显式--task90统一生成一次时间线。不重复本轮两modal/已有后台90与900秒/未变工具能力的hover、pressed、drag；下一步先核对人工环境答复及剩余真实输入/设备/前台条件，并将尚可安全准备的RC版本、说明和包级核对具体化；最终全RC只对实际冻结候选集中跑，不能用编译/模拟或Rosetta冒充硬件/真实辅助技术。[QA90](benchmarks/rc-round90-summary.json)。

## 上一轮进展（QA89）

- 当前运行时`b617049`，Mac input UI **3c0eb8c75fb905fa3e4c08295e7400e9ad9e2e251b01ede1415a0c1223aab00d**（43,810,848B），Core仍1146。将main参数解析/--version前4组纯契约断言完整移到cfg(test)的4个单元测试，原body字节不变；仅兼容契约API/dummy MappingFlow存储cfg(test)，未加dead_code豁免。实际页面/handler/持久化schema、Kit/Fast/lock未改。初Clippy16组死代码错误保留，修正test-only边界后最终fmt/strictClippy/216pass+2ignore/Intelcheck/ARMrelease/CLI通过。[交付范围](benchmarks/startup-contracts/final-scope.json)。
- 唯一[run41275489271](https://github.com/gloryhui/workbuddy-proxy/actions/runs/41275489271)绑定b617/attempt1，ARM124756717668/Intel124756717805/Win124756717921首轮全部success，Desktop/旧升级/Windowsnative三job skipped；Win215pass/2ignore、两Mac216/2ignore，真实host/各检查原log核对。三UI artifact仅服务器元数据，未下载ZIP/不当分发包。Windowsnative未来入口pin同步newWin artifact13071258074/sourceb617，Core冻结957不变；本轮未重新派发GDI。[CI](benchmarks/startup-contracts/ci/validation.json)。
- 本机新候选后台原生启动一次：2供应商/5模型/1真实HTTP请求及成本，统计行、供应商4+1模型、设置打开返回和正常关闭通过，两原JPEG1440×901。首图代理未知、第二供应商图运行中均保留；gpui.json缺失不证明从未启动（可正常关闭移除）。配置字节与1请求/成本全字段退出后相同，normal/1admitted/0drop/0writefailure，Core/driver0，UI只证PIDgone；签名仅signature变化，mutation flag为比较器自测。Corekernel保护，GUI环境隔离+deny-only guard，不虚称GUIkernel。[原生](benchmarks/startup-contracts/native-smoke.json)、[数据](benchmarks/startup-contracts/data-proof.json)。
- 历史1562.402ms/current3及全部7marker不删除：gpui.json是Proxy构造尾marker，不是首帧/可交互。无阶段trace/OS对照，不能归因于4断言或声称启动已优化。QA86/88 input3c19的CPU/900秒与QA87 Win cd1仍按原来源，**不重标3c0e、不重复后台90/900秒**。最终前台/启动/峰值/性能仍缺。[marker审查](benchmarks/startup-contracts/startup-marker-review.json)。
- 初push TLS失败后读远端仍daac再正常push成功；错误短语head_sha查询不算前置，随后精确b617查0并核对远端后唯一dispatch。所有本机进程结束，专用yau7jgxt经完整/即时预检精确清理48文件/19子目录、52,807,476B约50.36MiB，专用临时0；必要两基线合计97.14MiB及共享缓存保留，系统盘约35.67GiB。**不要重开已删bundle。**[清理](benchmarks/startup-contracts/cleanup.json)。
- **40帧剩0，发布待验15=R01 4/R02 4/R03 3/R04 4，Goal active。禁止自动开启旁白/系统辅助功能/切输入源，未公开发布。** QA89证据提交后显式--task89统一生成一次时间线。人工验收环境问题仍无回复：下一步先核对外部条件与可安全推进项，不重复未变CUA能力的hover/pressed/拖列或已完成后台样本；不得用新增低价值测试代替真实输入/设备/前台/签名缺口。最终完整RC待实际候选冻结后集中执行。[QA89](benchmarks/rc-round89-summary.json)。

## 上一轮进展（QA88）

- 当前运行时代码仍568/UI3c19/Core1146，本轮只补现有Fast最终候选后台长期驻留，没有新构建或CI。R01 CUA文档能力未变，hover/mouseMove、分步按下/松开、多点drag仍缺；静态接线未证明产品故障，不重复原失败尝试。已异步询问一次人工验收环境，未收到回答；不得推定允许改系统设置。[R01审查](benchmarks/fast-long-residence/r01-capability-review.json)。
- 新隔离App，64供应商/256模型/1014真实HTTP请求与成本/4096候选/搜索前缀/选0，10秒warmup后900.010秒61点全inactive、前台同96217、PIDbirth逐点一致，计时内无输入/截图/分析器/构建。UI CPU单核2.7244%，RSS首尾分钟中位112.867→109.406MiB（−3.461）；Core.1544%、21.281→17.859MiB。n1后台未见持续RSS增长，不签无泄漏/前台/最终性能，RSS下降可含paging/compression。[协议/原样本](benchmarks/fast-long-residence/preregistered.json)、[汇总](benchmarks/fast-long-residence/long-summary.json)。
- 结束后独立vmmap：UI footprint129.9M/OS生命周期peak130.0M，Core15.2M/16.1M；生命周期峰值含准备，非计时区间peak/匹配基线。signedUI190af5af对input3c19全非签名字节相同，code_mutation_detected为比较器自测；Corekernel保护已验，GUI只有隔离环境和deny-only真实WorkBuddy guard，不虚称kernel沙盒。[签名](benchmarks/fast-long-residence/signed-runtime-identity.json)。
- 前后无先行点击实际z→0/Backspace恢复4096/0，后置Cancel一次无白壳，原生close后UIgone；三原JPEG均1440×901及AX保留。所有配置字节、1014请求/成本全字段退出后保持，stats normal/1014admitted/0drop/0writefailure；Core/driver0，UI退出码未知。[数据](benchmarks/fast-long-residence/data-proof.json)。
- 专用`studio-v2-fast-long-hsi8d7mg`经完整及即时预检精确清理50文件/19子目录、55,017,151B约52.47MiB，专用临时0，所有本轮进程结束。必要两基线共97.14MiB和共享缓存保留，系统盘约35.69GiB；其他历史目录未动。**不要重开已删bundle。**[清理](benchmarks/fast-long-residence/cleanup.json)。
- **40帧剩0，发布待验15=R01 4/R02 4/R03 3/R04 4，Goal active。禁止自动开启旁白/系统辅助功能/切输入源，未公开发布。** 本轮证据提交后显式`--task 88`统一生成一次时间线。下一步不要再跑相同后台90/900秒；先取得人工/真实设备条件补R01/R02、有效前台与启动慢marker/最终峰值结论；最终全RC在实际候选冻结后集中执行。[QA88](benchmarks/rc-round88-summary.json)。

## 上一轮进展（QA87）

- 测试精简入口`67eaff3`已推：`windows_native_only=true`复用原成功CI的UI568/Windows `cd1a266c`与冻结957/Core `880016c6`，源码漂移、过期、run/source/下载大小SHA不匹配即失败。仅组合两程序临时布局跑既有两尺寸GDI/生命周期，无构建/安装或分发包。Mac UI仍3c19/Core1146，Windows hash不能混用。[预注册](benchmarks/windows-fast-native/preregistered.json)。
- 唯一[run41272932710](https://github.com/gloryhui/workbuddy-proxy/actions/runs/41272932710)绑定67eaff3/attempt1，job124749239785首次43秒成功，Desktop/UI构建/旧升级三job skipped；artifact13070162370、154,989B ZIP大小SHA与所有原报告/PNG均独立核对。[CI来源/清理](benchmarks/windows-fast-native/ci/validation.json)。
- 原PNG1442×901/1102×701已逐图查看，实际client1440×900/1100×700、DPI96，caption和toolbar无可见重叠，小窗筛选正常换行。仅英文1供应商/64模型/0请求，未验caption/托盘点击、键盘/IME/HiDPI/弹窗；Status列在小窗横向视口外，未验横滚。harness单实例、关设置留同Core、重开/草稿隔离、正常退出和统计run通过，harness/capture0，UI退出码未知。[原图](benchmarks/windows-fast-native/visual-review.json)。
- 临时显示1920×1080恢复1024×768校验通过，DPI未改。两精确自有runner临时根已清理205,385,996B约195.87MiB，runner专用临时0；本机未新建临时根，保留两必要基线共97.14MiB及共享缓存，系统盘约35.76GiB。无本机应用窗口/新后台进程；不要重开QA86已删bundle。首归档GET EOF只补读；初validator将GitLF对WindowsCRLF误判，精确展开hash一致，未改原报告/重跑CI。[QA87](benchmarks/rc-round87-summary.json)。
- **40帧剩0，发布待验15=R01 4/R02 4/R03 3/R04 4，Goal active。禁止自动开启旁白/系统辅助功能/切输入源，未公开发布。** QA87证据提交后生成时间线，首次遗漏`--task`保留旧86，随后显式87修正；不要重复相同验证。下一步优先R01可支持的真实缺口与R03有效前台/启动慢样本/最终长时证据；前台API条件此前未建立，不重复要求用户点击同一旧窗口，不把后台CPU结果当最终前台门禁。最终完整RC留实际候选冻结后统一执行。

## 上一轮进展（QA86）

- 运行时代码已推`5681103`：UI SHA256 `3c19fb96dd345c7e0a3287ee58eb523e5c959a5f555d57b888d51ec062893bbb`，Core仍`1146f58acdb163126ed03e4298e1d48dee43e620256ca52657d139ff4b742a4e`，Core源码未改。Kit仍0.7.3，仅启用可选Fast并精确pin引擎0.1.7；实际macro为0.1.12，Cocoa0.26.1→0.26.0、unicode-properties0.1.4→0.1.3为约束所需，详见锁图。实际registry包校验`35cb4c01...`且重要源文件逐字节等于原档，未改registry。缓存回放节点/焦点关系/动作，`retained_gpui::Window`编译期约束避免切回缺回放后端。[源码/范围](benchmarks/dependency-review/delivered-scope.json)。
- 试验首UI32beb发现Cancel清内容后留下白壳/遮罩，原图/AX/代码补丁保留。将`manage_dialog`从Proxy.render移到weak owner-state observe_in，F1/3c19同4096负载一次Cancel后无壳，统计1015可达。模型空ID错误保留modal、协议Esc、删除/同步/取消同步/端口Cancel均通过；StartupSync/pendingPort未触发，不签全状态。小图实际1100×701，恢复末图1440×900；蓝白→Studio、zh→en→zh通过。[红绿](benchmarks/dependency-review/modal-regression.json)、[F1原生](benchmarks/dependency-review/F1-native-final.json)。
- fmt、GPUI213pass/2既有ignore、strictClippy、Intelcheck、ARMrelease及最终CLI0。两个ignore的真实Core19发现/10创建沿用QA85未变业务源码证据，本轮未重复。最初离线依赖冲突、网络元数据失败、后端crate名称编译错误均保留，不把计时前错误当产品测试重跑到绿。
- 原前台P1在t0独立Cocoa发现inactive而停止，只有1点；C1-active只做前置等待，0计时。getApp后台启动、一次明确AX Raise也不让NSRunningApplication变active，用户手动就绪回复后仍观测前台ChatGPT；不反推原因/不伪造active、不重复询问同一前置确认。本轮已明确告诉用户不再要求保持前台，前台门禁仍缺。
- 单独预注册后台BGP1/BGC1，fresh UI73558/75756、同Core56285、64供应商/256模型/1015请求成本/4096候选/q前缀/选0；各90.005秒/19点全inactive、前台同宿主96217，前后实际z/Backspace无先行点击仍路由搜索。UI单核CPU6.6996→2.8221%，约降57.88%，Core均0.1555%。固定顺序n1，内部windowactive/遮挡未直接测，RSS不是footprint/峰值/泄漏；不签前台、启动或长期驻留。500ms光标与负载未改。[原样本/对照](benchmarks/dependency-review/background-comparison.json)。
- 首32beb弹窗保持4096/0时真实单条HTTP使后台1014→1015自动刷新，无手动Refresh；这条证据不重标为F1。原1014及新增后全1015请求/成本退出后逐字段保持，Core run normal/1015 admitted/0drop/0writefailure。语言往返补384既有typeddefaults（64protocol、64完整settings、256capsnull），所有原字段保持，配置字节不同。Core/driver0，CUA拥有UI退出码未知只证PIDgone。[数据](benchmarks/dependency-review/data-proof.json)。
- 本轮专用`studio-v2-dependency-review.jy_z2w91`已按完整和即时父链/uid/realpath/无symlink/mount/库存/lsof0/ps0预检精确非强制清理，76文件/44子目录、276,250,288B约263.45MiB，本轮专用临时0、系统盘约35.66GiB。新增必要旧957 UI4658/Core1146参考`gpui/target/studio-v2-baselines/9570451-arm64-release-46585f86`约48.59MiB，连原a6e参考合计101,859,410B约97.14MiB；共享缓存保留。旧QA84边界外Chrome socket未动，未重测当前大小。**不要重开已删除bundle；禁止自动开启旁白/系统辅助功能或切输入源。**[清理](benchmarks/dependency-review/cleanup.json)。
- 新UI实际改动，R04.1按最终候选重新pending，历史957全CI与原安装测试保持其来源；当前发布待验**15项=R01 4/R02 4/R03 3/R04 4**，40帧仍签收/剩0，Goal active，未公开发布。新阶段CI仅UI-only，绝不当最终Core/打包全矩阵。
- **CI先追原状态，不重复派发。** Source5681103首次workflow派发在GET workflow时EOF；之后API已确认同source无run，才补一次派发。**已确认唯一run41270728784绑定5681103/attempt1，Win/ARM/Intel三UI job首轮全部success，Desktop和旧升级skipped；[原log/元数据/逐项验收](benchmarks/dependency-review/ci/validation.json)已归档，Win212pass/2ignore、两Mac213pass/2ignore，三真实toolchain host分别x64Windows/ARM/x64Mac，UI artifact只核服务器元数据未下载ZIP。** 最终首次成功的是已核对workflow ID的普通POST；此前两次仅GET解析EOF，逐次API确认无run，未创建重复run。当前具体run/退出码见`benchmarks/dependency-review/dispatch.json`；网络进程句柄需按实时工具输出，不能仅凭文件当活着。本轮唯一run终态/原log/metadata已核对；QA86时间线已在证据提交后统一生成一次；本轮不再重跑updater。下一步仅补实际缺口，新增渲染后的Windows原生GDI代表页可复用既有脚本集中验证，最终全RC留正式候选，不重跑未变Core全套。R01未支持的旧drag/hover/pressed/逐引擎IME/真实AT仍缺，不重复能力未变的尝试。

## 上一轮进展（QA85）

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
