# Studio V2 会话交接与恢复状态（2026-11-01）

此前用户要求暂停并换会话；2026-11-01已按本轮明确请求恢复Goal为active，仍从R01推进。恢复动作已核对Git/远端/独立Goal；禁止自动开启旁白。以下旧轮证据保留原来源，不能混称当前候选。

## 本轮恢复进展（QA76）

- 最终候选SHA256 `631599c2bc50295cb8164ed6be583974b7a3d01bfb7568739e0655d25133ccfd`；GPUI207pass/2既有ignore、fmt、strict Clippy、Intel类型检查、arm64release通过。源码绑定见[构建](benchmarks/r01-resume/build-final.json)。
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
