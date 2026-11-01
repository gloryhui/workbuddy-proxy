# Studio V2 正式版批次计划

本文件由 release_plan.json 生成；修改计划数据后运行 scripts/update_studio_progress.py。

当前批次：R01；Goal：active。主体已有实现和分项证据，界面与正式版验收以实际签收为准。

## 口径与顺序

- 主体已实现/历史分项已验证：已有代码和证据，不等于新设计或最终发行包通过。
- 40个界面按B01–B05逐帧记录最新设计、完整路径和两尺寸的界面验收；R01汇总签收。平台/性能/安装门禁独立记录，不能让同一全局待办重复阻塞40次界面施工。
- 正式发布必须同时满足40帧界面签收和R01–R04所有全局门禁。界面数量归零仍不等于正式版可发布。
- 核对布局/资产/字号角色/层级/可见状态；原生ICC/AA差异明记，不新增逐字形PostScript、字体文件版本或全RGB字节相等门禁。
- 数据丢失/崩溃/错误保存为P0；阻断流程/误导结果/不可达为P1；实际可见布局或状态差异为P2。P0/P1先修，P2同批集中修，合理原生栅格差异留说明。

执行顺序：P00 → B01 → B02 → B03 → B04 → B05 → R01 → R02 / R03 → R04。

## 批量推进规则

- 按B01→B02→B03→B04→B05集中施工；R01统一关闭界面与共享状态，再做R02/R03，最后R04。
- 每批先列完整差异并连续完成全部小任务；共享布局/组件一次改，避免一个间距一次构建。
- 每批复用候选程序、隔离样本和应用会话，冻结后集中完成真实交互、全部相关画板两尺寸原图与性能对照。
- 集中验收后一次列问题；目标一轮验收加一轮差异复验。未过则继续修，不以轮数上限冒充通过。
- 只复测受改动影响的路径和共享回归；候选SHA/来源差异要清楚，最后RC统一跑全套。
- 功能批次通过后提交并推送，更新一次进度HTML；编译、单个像素和中间候选只留证据，不刷新时间线。
- 后台优先读取，不主动强制置顶；需真实输入/系统场景时集中安排；Mac锁定则先写代码、待可操作再验。
- 严格保留原代理核心、配置/数据与全部现有功能。专用临时进程结束后精确清理，复用共享构建缓存。
- 按本轮用户要求精简验证：根据改动影响选择检查，已通过且未受影响的本机检查不重复；UI-only CI跳过Core/安装/迁移。正式RC最后集中核对完整门禁，精简入口不替代全矩阵、真实平台或最终性能验收。
- 按影响范围精简验证：QA86渲染后端变更集中补相关原生modal/输入/尺寸/偏好及三平台UI阶段检查；最终完整RC一次集中执行。全局门禁按实际新候选重开，历史证据不重标。

## P00：最新 Figma 基线核对

2026-10-31官方Figma连接已恢复；最新51:2仍为同组40节点，B01的19份完整高保真上下文/截图已读取并建立基线。用户已恢复Goal并要求按B01整批推进。

1. 用户完成Figma连接并明确继续后，读取最新51:2全画板清单/高保真上下文和截图。
2. 比较本计划40个节点与最新画板，新增/删除/变更单列；按共享组件一次审查资产及可复用范围。
3. 锁定本批设计版本与源码SHA；变更中的画板先排依赖，不能拿旧稿宣称最新一致。

通过标准：可读最新设计且画板/资产/批次映射已核对；此时才能开始B01。

## B01：供应商管理完整旅程

覆盖画板：54:2、66:320、66:538、66:760、67:338、67:548、78:364、78:605、78:843、78:1080、78:1325、78:1804、79:400、79:885、79:1102、80:418、80:646、80:864、80:1114。

已有成果：冻结2a12：最新19份完整设计、38张两尺寸主图及补图；供应商完整CRUD/取消/读回/容量/搜索/连接200与503、独立字段和历史保护通过。四轮原负载0，内存与启动标记增量明确保留R03。

本批交付：一次收口19个状态画板：从列表→新增→保存→编辑→复制→删除→满载搜索完整走通；不再拆成单个输入框回合。

### B01.1 列表、选中、容量与搜索

- 已完成/已有证据：本批真实0/1/2/64状态、27/64/无匹配/清空、尾项小窗选中保持；64时实际点新增/复制不产生草稿，Core字段不变。
- 剩余工作：本功能批次已收口；全局输入/全部控件状态/平台/最终性能与安装发行按R01–R04独立验收。
- 验收标准：两个尺寸依次验证0/1/2/64、搜索首尾/无匹配、清空、选中可见；64时新增/复制禁用，既有数据不变。
- 历史记录：UI-4、UI-29、UI-32、UI-48、UI-58、UI-68。

### B01.2 新增草稿到保存完成

- 已完成/已有证据：同候选新增取消、非法URL拒绝、合法loopback保存只增1家/0模型、保存态两尺寸、删除恢复2/5；Core契约补失败与重试边界。
- 剩余工作：本功能批次已收口；全局输入/全部控件状态/平台/最终性能与安装发行按R01–R04独立验收。
- 验收标准：取消不写Core；合法保存只新增一次；失败留草稿；重开读回原字段，已有供应商不变。
- 历史记录：UI-26、UI-27、UI-28、UI-30、UI-68。

### B01.3 编辑资料与未保存保护

- 已完成/已有证据：A/B改名取消恢复，A实际保存、切B回A读回并还原；原始目标字段与14历史保持，长标题省略但字段/侧栏完整可读。
- 剩余工作：本功能批次已收口；全局输入/全部控件状态/平台/最终性能与安装发行按R01–R04独立验收。
- 验收标准：编辑→取消读回不变；编辑→保存只改目标；切换不静默丢草稿；Key不写入截图或日志。
- 历史记录：UI-26、UI-39、UI-46、UI-60、UI-61、UI-68。

### B01.4 复制草稿到复制完成

- 已完成/已有证据：复制取消与保存三态两尺寸通过：3家9模型且源2家全字段保持，复制4模型非身份字段保持；沿用原草稿Key继承语义（空仍为空），没有新增清空逻辑。
- 剩余工作：本功能批次已收口；全局输入/全部控件状态/平台/最终性能与安装发行按R01–R04独立验收。
- 验收标准：只在确认保存时新增副本；原供应商与模型全部字段保持；复制后连接徽章不借用源对象。
- 历史记录：UI-27、UI-28、UI-30、UI-59、UI-68。

### B01.5 删除两家供应商与失败恢复

- 已完成/已有证据：A/B确认框两尺寸、Cancel/X、新鲜B弹窗Esc、默认Return不误删；分别删除后幸存对象原字段保持，再至空态；遮罩阻断语言入口。删除故障恢复沿用UI32/33未改处理器，R01统一最终故障矩阵。
- 剩余工作：本功能批次已收口；全局输入/全部控件状态/平台/最终性能与安装发行按R01–R04独立验收。
- 验收标准：Cancel/X/Esc不删；显式确认仅删目标；失败后与Core对齐且保留其他草稿；无隐式同步/重启。
- 历史记录：UI-25、UI-32、UI-33、UI-64、UI-68。

### B01.6 协议/预设/思考菜单与Key显隐

- 已完成/已有证据：协议/预设Bedrock搜索与方向高亮/Key显隐/思考菜单两尺寸完成整帧首审；四轮同序实际预设原项确认、Low→Follow显示变化。原MenuState/选择delegate/回调字节保持；完整键盘/IME/AX仍属R01。
- 剩余工作：本功能批次已收口；全局输入/全部控件状态/平台/最终性能与安装发行按R01–R04独立验收。
- 验收标准：真实选择/搜索/清空/无匹配、Esc/外点、Enter/Space、窄窗可达；显隐切回遮罩，事件语义不变。
- 历史记录：UI-31、UI-39、UI-60、UI-61、UI-65、UI-68。

### B01.7 连接测试状态及整批版式

- 已完成/已有证据：真实本地200/SSE、503失败与恢复；38张主图+9补图，同候选四行小窗表内滚到完整末行另有原图。四轮原负载均0；busy视觉已验，但CUA失效引用导致点击拦截未实证，明确留R01，未虚报通过。
- 剩余工作：本功能批次已收口；全局输入/全部控件状态/平台/最终性能与安装发行按R01–R04独立验收。
- 验收标准：正确对象才显示有效成功；改字段隐藏旧结果；测试失败不变绿；19帧共38张实际运行原图及Core字段证明。
- 历史记录：UI-58、UI-59、UI-60、UI-61、UI-68。

## B02：模型管理与四种协议

覆盖画板：59:11、59:279、61:289、61:561、61:838、61:1111。

已有成果：最终87db模型与四协议6帧签收：完整真实往返与容量、正确格式两尺寸原生PNG、四轮原负载及历史保护通过；原JPEG/阴影问题与补拍来源、默认序列化、性能小幅成本均单列。

本批交付：集中完成6张弹窗和模型整条管理链，复用一套表单/模态组件，保留全部真实字段。

### B02.1 模型发现、选择与容量

- 已完成/已有证据：真实38/4096发现、503/空结果、搜索/无匹配/清空/全选反选/仅看勾选、已有项点击仍只读；beam-free取消/应用草稿/父取消/显式保存读回，256满载点击拒绝超量。精准hover及聚焦Apply的Return留R01。
- 剩余工作：本功能批次收口；完整焦点/IME/AX/精确悬浮、主题/平台及长时性能/安装发布仍按R01–R04独立验收。
- 验收标准：4096候选可滚到末尾；已有模型不可重复；选择取消不写Core；确认仅进草稿，显式保存才写入。
- 历史记录：UI-20、UI-49、UI-51、UI-63、UI-69。

### B02.2 模型表多选与键盘

- 已完成/已有证据：同版实际范围选中、Return/F2打开原目标、局部删除及父取消恢复；主夹具Cmd视觉不确定，满载明确fill-249/250范围、Cmd取消249后仅删除250，取消恢复255。
- 剩余工作：本功能批次收口；完整焦点/IME/AX/精确悬浮、主题/平台及长时性能/安装发布仍按R01–R04独立验收。
- 验收标准：首末行、Shift范围/Cmd切换、方向键/Return/Space、长值提示可读；删除只作用于原目标且可取消。
- 历史记录：UI-40、UI-44、UI-47、UI-49、UI-51、UI-54、UI-58、UI-69。

### B02.3 能力编辑与默认思考

- 已完成/已有证据：AUTO点击→手动→Space恢复；六能力/五档与默认Low、非法输出-1、合法名称/能力/四限制应用与保存读回；小窗全部下方字段和固定页脚实见。
- 剩余工作：本功能批次收口；完整焦点/IME/AX/精确悬浮、主题/平台及长时性能/安装发布仍按R01–R04独立验收。
- 验收标准：非法限制拒绝；合法确认回草稿；Save读回；取消不改；默认思考/手动覆盖和未知能力不被UI重绘覆盖。
- 历史记录：UI-21、UI-44、UI-50、UI-51、UI-53、UI-69。

### B02.4 四协议共享版式集中对齐

- 已完成/已有证据：六份最新完整设计一次对齐；最终87db的6×2真正原生PNG及两张小窗底部图通过，首次CUA JPEG/阴影PNG格式问题诚实保留并用全新同版夹具补拍，未转换冒充。
- 剩余工作：本功能批次收口；完整焦点/IME/AX/精确悬浮、主题/平台及长时性能/安装发布仍按R01–R04独立验收。
- 验收标准：四张各两尺寸，字段/按钮/错误可达；遮罩阻挡背后工具栏；关闭返回原焦点；不删字段贴静态图。
- 历史记录：UI-24、UI-62、UI-64、UI-69。

### B02.5 四协议真实数据往返

- 已完成/已有证据：Chat/Responses及Anthropic预算/Adaptive、Gemini预算/level实际非法拒绝→合法Apply不落Core→显式Save→读回；目标之外原字段/历史保持，源码定义默认序列化补全单列。
- 剩余工作：本功能批次收口；完整焦点/IME/AX/精确悬浮、主题/平台及长时性能/安装发布仍按R01–R04独立验收。
- 验收标准：各协议独有认证/输出/预算/映射按真实校验；Apply不直接写Core；显式保存只改目标并保留其他模型。
- 历史记录：UI-24、UI-62、UI-69。

### B02.6 模型与协议整批回归

- 已完成/已有证据：最终候选五编译门禁、Main/Capacity/单独原生补拍及所有生命周期通过；原1000小请求/64模型/80×1MiB并发4的四轮0，全部成本保留R03；200份证据归档及本轮专用目录安全清理。
- 剩余工作：本功能批次收口；完整焦点/IME/AX/精确悬浮、主题/平台及长时性能/安装发布仍按R01–R04独立验收。
- 验收标准：同一候选全链路与6帧12张原图；原Core路由/能力/兼容字段无回退，性能结果不混用旧候选。
- 历史记录：UI-20、UI-21、UI-24、UI-62、UI-63、UI-64、UI-69。

## B03：统计、价格与本地用量

覆盖画板：52:2、56:8、57:8、58:8、60:249、60:475、69:359。

已有成果：四统计页面、筛选/分页、价格/历史快照、采集/本地导入均已落地；概览同版完整回归已做。

本批交付：合并7个画板和统计业务链；优先补未闭环行为，集中修正所有表格/筛选/浮层差异。

### B03.1 概览最终差异与真实数据

- 已完成/已有证据：QA70：最新概览两尺寸与1000→1001自然刷新、503/缺失用量已验；overview/trends与UI67字节一致，币种选择复用旧实测，当前混合币种快照另核。
- 剩余工作：本批界面范围已签收。原生列拖动未证实、精确hover/完整键盘IME/AX及平台长期性能按R01–R04独立继续；不冒称全部发布门禁通过。
- 验收标准：未知缓存/费用保持—；自然刷新与手动刷新分别记证据；设计结构/资产一致，保留真实数值。
- 历史记录：UI-10、UI-11、UI-12、UI-66、UI-67。

### B03.2 用量明细、分页与长列表

- 已完成/已有证据：QA70：1000真实HTTP记录、10/20/50/100、前后/合法及非法跳页/6.5s草稿、20条第2页首ID服务端对照、两尺寸独立内滚与首末列可达。
- 剩余工作：本批界面范围已签收。原生列拖动未证实、精确hover/完整键盘IME/AX及平台长期性能按R01–R04独立继续；不冒称全部发布门禁通过。
- 验收标准：至少1000条合成真实请求，服务端分页；表体独立滚动且表头/分页固定；两尺寸能看所有列；页码草稿不被tick抢走。
- 历史记录：UI-13、UI-14、UI-55、UI-56、UI-57。

### B03.3 汇总分析与范围浮层

- 已完成/已有证据：QA70：2供应商/5模型与550000总量/500000输入/50000输出/400000缓存子集、范围浮层及小窗下卡已验；卡片比例/间距按最新稿。
- 剩余工作：本批界面范围已签收。原生列拖动未证实、精确hover/完整键盘IME/AX及平台长期性能按R01–R04独立继续；不冒称全部发布门禁通过。
- 验收标准：分组计数和范围总量与只读API/SQL相同；缓存比例不混加；小窗横向列与下卡均可达。
- 历史记录：UI-15、UI-56、UI-57。

### B03.4 价格新增/编辑/删除与历史快照

- 已完成/已有证据：QA70：新建→CNY请求→USD修订/请求→删除/新请求→新ID恢复/请求、503目录更新失败/重试；原全部请求与成本不变。原菜单直接删除，无确认取消分支。
- 剩余工作：本批界面范围已签收。原生列拖动未证实、精确hover/完整键盘IME/AX及平台长期性能按R01–R04独立继续；不冒称全部发布门禁通过。
- 验收标准：规则revision/币种/费率只改目标；新请求用新价，历史全字段快照不变；取消/失败不写入。
- 历史记录：UI-16、UI-18、UI-63、UI-64、UI-65。

### B03.5 请求详情与全部缺失语义

- 已完成/已有证据：QA70：成功/503/无usage/导入详情及More、真实价格快照/未知HTTP耗时、144字符模型、直接Esc及子层关闭等待1.2s后父层Esc通过。
- 剩余工作：本批界面范围已签收。原生列拖动未证实、精确hover/完整键盘IME/AX及平台长期性能按R01–R04独立继续；不冒称全部发布门禁通过。
- 验收标准：详情与原始记录逐字段一致；未知显示—，导入不伪装HTTP200；更多/Esc只关闭当前层。
- 历史记录：UI-19、UI-45、UI-63、UI-64。

### B03.6 采集、保留和本地导入

- 已完成/已有证据：QA70：币种/保留180→90→180/采集OFF转发200且不增行/ON；合成JSONL实际5Builtin+1Custom、暂停→恢复、去重、权限读错→修复自然恢复至7Builtin+1Custom。
- 剩余工作：本批界面范围已签收。原生列拖动未证实、精确hover/完整键盘IME/AX及平台长期性能按R01–R04独立继续；不冒称全部发布门禁通过。
- 验收标准：仅访问合成JSONL；重复导入不增重复；保留策略不误删范围外数据；错误可恢复且不改真实用户文件。
- 历史记录：UI-17、UI-65。

### B03.7 统计整批验收与数据不变

- 已完成/已有证据：QA70：最终4b同候选7帧14原生PNG、1014请求/1006成本全字段保护、Core配置字节不变及P/C/C/P原负载短测；输入/平台/长期性能仍R01–R04。
- 剩余工作：本批界面范围已签收。原生列拖动未证实、精确hover/完整键盘IME/AX及平台长期性能按R01–R04独立继续；不冒称全部发布门禁通过。
- 验收标准：同一候选、两尺寸、字段摘要/数量/金额读回一致；新旧原负载对照；仍有全局待办单列，不反复清零页面结果。
- 历史记录：UI-55、UI-56、UI-57、UI-63、UI-65、UI-66、UI-67。

## B04：工具栏、窗口、语言与外观

覆盖画板：62:308、64:335、104:1378、104:1605、104:1823、104:2046。

已有成果：QA71：六帧12原生图、语言失败回滚修复、三语言/七皮肤、真实隔离开机启动失败重试、更新状态/迟到关闭、恢复窗口与遮罩及四轮匹配短测签收。

本批交付：六张画板与全部工具按钮一次连续验收；窗口与菜单共享问题集中修。

### B04.1 七功能布局、拖动与遮罩

- 已完成/已有证据：QA71：原top8/right32、300宽、7×36/20图标/8间距和全部七事件字节保持；两尺寸图及模态遮罩下点击语言不打开菜单。
- 剩余工作：B04界面范围已签收；完整忙态拦截/键盘IME/辅助功能/主题矩阵由R01集中验证，系统登录/标题拖动/全屏/外链实际启动及其他平台由R02验证，长期性能和发行仍R03/R04。
- 验收标准：两尺寸位置一致、标题不动；七按钮均可达；拖动不吞点击；遮罩高于工具栏；Windows冲突交R02闭环。
- 历史记录：UI-1、UI-23、UI-41、UI-43。

### B04.2 语言菜单与持久化

- 已完成/已有证据：QA71：旧版503后翻译/勾选不一致已原生复现，新版失败恢复已保存语言；English→系统实际中文→中文落盘，已选不重复写、筛选/Low草稿保持，英文价格对话框取消。
- 剩余工作：B04界面范围已签收；完整忙态拦截/键盘IME/辅助功能/主题矩阵由R01集中验证，系统登录/标题拖动/全屏/外链实际启动及其他平台由R02验证，长期性能和发行仍R03/R04。
- 验收标准：所有页面/每页条数/错误/弹窗即时翻译；选中不重复写；草稿/筛选不丢；Tab/方向/Return/Space/Esc正常。
- 历史记录：UI-14、UI-34、UI-41。

### B04.3 六旧皮肤与Studio入口

- 已完成/已有证据：QA71：Mono/Blue/Violet/Rose/Amber/Teal/Studio逐个真实选择读回；外观落盘失败保留Studio，修复后字节恢复。Mono无菜单和Studio七入口两尺寸完成。
- 剩余工作：B04界面范围已签收；完整忙态拦截/键盘IME/辅助功能/主题矩阵由R01集中验证，系统登录/标题拖动/全屏/外链实际启动及其他平台由R02验证，长期性能和发行仍R03/R04。
- 验收标准：原皮肤键与默认不变；选择落盘/重开一致；失败保留原皮肤；页面和草稿不因换肤丢失。
- 历史记录：UI-8、UI-38、UI-41、UI-57、UI-65。

### B04.4 开机启动与恢复窗口

- 已完成/已有证据：QA71：真实隔离plist ON→OFF→失败回滚OFF→修复ON→OFF；实际1100恢复1440、再缩1100仍保留目标提示，两尺寸完成。
- 剩余工作：B04界面范围已签收；完整忙态拦截/键盘IME/辅助功能/主题矩阵由R01集中验证，系统登录/标题拖动/全屏/外链实际启动及其他平台由R02验证，长期性能和发行仍R03/R04。
- 验收标准：隔离plist与UI真值一致；失败不假成功；恢复1440×900并退出全屏；不改真实LaunchAgents或打断用户会话。
- 历史记录：UI-35、UI-36、UI-41。

### B04.5 检查更新与外链

- 已完成/已有证据：QA71：受控latest/available0.5.8/403/无效/等待关闭迟到响应，额外两次受控latest原生预采样拍到Checking；独立真实Core公网检查返回当前最新0.5.7；force和两个原外链处理器保持。
- 剩余工作：B04界面范围已签收；完整忙态拦截/键盘IME/辅助功能/主题矩阵由R01集中验证，系统登录/标题拖动/全屏/外链实际启动及其他平台由R02验证，长期性能和发行仍R03/R04。
- 验收标准：状态对应真实响应；busy不重复提交；关闭后不被迟到反馈重开；外链只打开预期仓库/Issue地址。
- 历史记录：UI-1、UI-37、UI-41。

### B04.6 工具与窗口整批验收

- 已完成/已有证据：QA71：同d08候选6×2原生图、14→15真实回环请求/费用全字段保护、仅声明语言/开机/皮肤写入；P/C/C/P原负载四轮同条件短测。
- 剩余工作：B04界面范围已签收；完整忙态拦截/键盘IME/辅助功能/主题矩阵由R01集中验证，系统登录/标题拖动/全屏/外链实际启动及其他平台由R02验证，长期性能和发行仍R03/R04。
- 验收标准：一次完整工具巡检且所有状态有证据；不显式强制置顶，记录实际验证边界；原代理配置不变。
- 历史记录：UI-23、UI-34、UI-35、UI-36、UI-37、UI-38、UI-41、UI-65。

## B05：同步、日志与进程生命周期

覆盖画板：60:27、68:350。

已有成果：冻结78：两份最新设计/4张两尺寸原生PNG；草稿同步/移除、冲突和损坏文件保护、受控真实重启/端口部分成功、日志阅读修复/复制清空、正常冷启动及用户物理托盘验收通过。四轮原负载0，峰值+3.33MiB留R03。

本批交付：完成对外集成边界与设置窗口关闭/重开整条链，真实业务数据保持。

### B05.1 同步/取消同步确认与范围

- 已完成/已有证据：2帧4原图、Cancel/X/Esc/neutralReturn/遮罩/英文检查；当前A草稿保存后同步全部5模型，B草稿不提交；手工同ID冲突正确拒绝覆盖，移除恢复原2条手工配置。
- 剩余工作：本功能批次已收口；最终输入/无障碍、实际系统/客户端、长期性能和安装发布按R01–R04独立验收。
- 验收标准：仅在显式确认后写隔离WorkBuddy；取消不写；安装条目与文件ID一致；无隐式应用。
- 历史记录：UI-22、UI-63、UI-64。

### B05.2 WorkBuddy/端口重启与失败回滚

- 已完成/已有证据：原native FFI在唯一受控Cocoa目标上真实重启、20s拒绝退出、OS重开失败/修复；端口0/65536/取消/占用拒绝，64645→50428→50695同Core监听迁移及重启失败部分成功通过。不是实际已安装WorkBuddy互操作。
- 剩余工作：本功能批次已收口；最终输入/无障碍、实际系统/客户端、长期性能和安装发布按R01–R04独立验收。
- 验收标准：原用户会话不被随意结束；每阶段结果可判定；失败报告准确、配置可恢复，不把脚本成功当产品路径通过。 端口0/65536拒绝，取消不重启；合法变更后真实端点一致，失败回滚或部分成功提示符合原语义。
- 历史记录：UI-22、UI-42。

### B05.3 日志阅读、清空与复制

- 已完成/已有证据：仅修Rust日志视图：100条过期后保留可见锚点，宽/小窗单条与3条批量、idle及底部跟随；原生复制100行与手工选区一致，200/503/200、清空ring0/新请求恢复，SQL历史保留。
- 剩余工作：本功能批次已收口；最终输入/无障碍、实际系统/客户端、长期性能和安装发布按R01–R04独立验收。
- 验收标准：日志从旧到新、仅跟随底部；无新日志不抢位置；复制与显示一致；不含Key，保留上限。
- 历史记录：UI-7。

### B05.4 真实代理卡、端口、托盘与单实例

- 已完成/已有证据：地址原生复制/运行卡真实2/5/请求/端口；正常冷启Core+Tray+UI、关窗转发、用户手动物理Settings重开唯一UI并恢复B草稿、用户物理Quit三角色全退出。CUA仅操作窗口与观察，非CUA托盘自动化。
- 剩余工作：本功能批次已收口；最终输入/无障碍、实际系统/客户端、长期性能和安装发布按R01–R04独立验收。
- 验收标准：关设置释放UI，代理继续转发；重开唯一UI且草稿正确；退出关闭三角色，无僵尸/重复实例。 状态/计数/端口来自真实Core，不复制假地址、不重复显示三份运行信息。
- 历史记录：UI-7、UI-42、UI-52。

### B05.5 同步与生命周期整批验证

- 已完成/已有证据：同冻结78完成同步→移除→关窗/重开/退出和4张正式图；原14请求/成本全字段保护至156。206通过/2既有忽略及其他构建门禁0；P/C/C/P原负载0且各2090条持久化，专用临时根已精确清理。
- 剩余工作：本功能批次已收口；最终输入/无障碍、实际系统/客户端、长期性能和安装发布按R01–R04独立验收。
- 验收标准：配置/历史无无关改动；每个子进程退出或继续运行符合动作；共享缓存保留，专用临时目录准确清理。
- 历史记录：UI-7、UI-22、UI-42、UI-52、UI-63、UI-64。

## R01：全局状态、输入与无障碍

覆盖画板：全局发布门禁，不新增画板计数。

已有成果：40帧/80主图集中签收完成；此前表格语义、busy、21主题语言与真实Qingjian/Unicode冷读归档。QA76已恢复Goal，修复并补验外部save64活动表/modal延后/dirty草稿重基准及冷恢复、末页分页焦点；全状态slot/真实拖宽/当前逐引擎IME/真实AT仍缺，R01保持部分完成。 QA77补原生组件适用状态清单、下拉项具名语义、模型行完整身份/选择文字、弹层Dialog/modal元数据；原生选择/取消、空表及小窗滚动补验，仍不签收完整R01。 QA81补真实旧数据的自定义731天/OBSERVE/Unicode/价格与13条分页两尺寸读回；共享全状态/slot/列drag/当前逐引擎IME/真实AT仍待。

本批交付：对共享组件统一验收，不让同一键盘/主题待办在40帧中重复执行。

### R01.1 共享状态与层级矩阵

- 已完成/已有证据：e1f候选真实held Test：重复点击未增加提交，精确save/test各1；controlled update各1，完成不重开已隐藏通知，16条历史完整。 最终631599真实外部save64在model modal期间保持原4条/原字段，Cancel后poll刷新64/总65；dirty名称与64模型跨外部rename、切换和原生冷重开保持。外部配置变化后重写私有draft base，14请求/成本逐字段不变。 QA77将36wrapper/118限定调用点归为10个适用状态族，记录代表性原生idle/focus/disabled/empty/checkbox/overflow；pressed/Tooltip等不满足的证据明记。 QA80最终4658投影缓存与条件列表通知通过213本机测试/实际选择、仅选2→1→0、反选/清空、搜索4095/无匹配、关闭重开及应用主题语言往返；长显示名在真实单元格确实截断，但正常单击等待未显示完整Tooltip，AX全名不作提示出现证明。 QA81实际旧版数据读回补自定义731天（5个预设未误高亮）、OBSERVE、手工USD2/8、持久13条/第2页3条两尺寸与滚动状态。原图外框1440×901/1100×701，不裁剪重标为900/700；没有全slot/pressed/hover证明。 QA84 actualsearch0match/restoration, invert4096overlimitwarning/restoration0 in64/256/1014 fixture captured; a state subset, notwholeapplicablematrix/slot acceptance. QA86新候选5681103/UI3c19：Fast0.1.7回放缓存a11y树/动作，32beb首次Cancel仅清body留下白壳，改为owner状态observe_in处理modal生命周期后，F1发现/编辑校验/协议Esc/删除/同步/取消同步/端口取消均回主页面，无残壳；StartupSync/pendingPort未触发，不签全状态/slot。 QA90当前b617/UI3c0e补齐StartupSync与pendingPort两个此前未验分支：隔离HOME受管旧地址由真实Core检查1/1 mismatch，原端口61270/拟61393确认均在1440×901与1100×701原图可读；中性Return后两提示仍在，Startup点击Cancel、pending按Escape均无壳/遮罩，重开编辑器仍61270。配置/整份合成models.json（含unrelated）/1请求成本退出后不变，UI只读allowlist、install/set_port/nativeRestart均0。 QA92当前Mac人工报告：首行供应商截断文字悬停约2秒显示完整名称且可读；只覆盖此cell，未捕捉提示帧，其他hover/pressed/slot不扩展签收。
- 剩余工作：供应商cell完整Tooltip已有QA92人工报告；其余Tooltip、hover/pressed及完整适用状态/实际slot仍待。QA90两重启modal已补，不重列未触发；不把AX/缓存/编译或单cell人工报告当整矩阵签收。
- 验收标准：共享按钮/输入/选择/列表/表格/弹层/Tooltip状态表完整；所有出现位置有正确slot和实际尺寸，无P0/P1。
- 历史记录：UI-41、UI-53、UI-59、UI-60、UI-61、UI-64、UI-75、UI-76、UI-77、UI-80、UI-81、UI-84、UI-86、UI-90、UI-92。

### R01.2 完整键盘与焦点恢复

- 已完成/已有证据：实际Tab/ShiftTab、编辑/选择/取消、四协议字段和分页往返已补证；中性Delete确认Enter不执行删除；不同候选来源明确。 最终631599复现修复Next禁用后的ShiftTab/Return回首页和Prev禁用后Tab退出；AX仅报window焦点，结论依原图和真实键盘行为，鼠标路径未强制重定向。 QA84 realTabx2/Space inpicker toggles0to4096to0 twice, query/4096preserved; exactnativeAXfocusedwidget still unavailable butnontextkeyboardrouting proved. FinalCancel/windowclose actual, no apply/save. Columnwidthdrag gap unchanged. QA86新候选F1模型ID空值Apply显示错误且modal保留，Cancel关闭；协议Esc及五类确认取消后后台控件可用。两后台样本前后无需重新点击，实际z/Backspace仍路由搜索；不等于完整物理键盘/IME或精确AX焦点身份。 QA88同Fast3c19/4096弹窗后台900秒后，无先行点击实际z→0、Backspace恢复4096/选0，Cancel一次无白壳/遮罩，正常close；不是完整键盘/IME、精确AX焦点或拖列验收。 QA90两危险重启确认在窗口AX焦点下Return后保持，Cancel/Escape关闭且端口恢复原值，独立audit无install/set_port/重启；AX不能确认精确子控件焦点，不作完整焦点/键盘门禁通过。 QA92用户明确供应商列手动拖宽、端口56290刷新及缩窗后仍保持；新原图1273×810确认缩窗，首drag图仍1440×901且边界近原约574，缩后约560受自适应重排影响。保留人工报告与截图不一致，不断言独立量化拖宽；未用CUA两点drag冒充真实成功。
- 剩余工作：QA92已有真实人工拖列/刷新缩窗保持报告，但前后图未独立量化拖宽，观察差异保留，不能假装该差异已解决；完整焦点回归仍待。旧固定时间列与能力未变的CUA两点drag失败不重复，不据此唯一归因产品。
- 验收标准：无键盘死路/陷阱/误提交；焦点可见、返回合理；关闭×Enter不触发业务确认，危险确认不默认执行。
- 历史记录：UI-40、UI-41、UI-50、UI-51、UI-62、UI-64、UI-67、UI-75、UI-76、UI-77、UI-84、UI-86、UI-88、UI-90、UI-92。

### R01.3 中文IME、Unicode与编辑撤销

- 已完成/已有证据：当前Qingjian逐键nihao产生真实marked text，Space落字你好，第一Esc取消合成、第二Esc关编辑器；Return落ASCII nihao未误确认。名称/模型/URL/Key/搜索/备注Unicode撤销重做和非法数字校验已抽查。模型名中文/emoji/组合字符一次保存，由独立Core/UI冷读逐码点一致，恢复原名后16条历史及成本保持。 QA81官方v056/057真实Core产生中文/emoji/组合重音配置、价格和12历史，当前CI包冷交接保留原字段与配置字节，原生UI可读。没有新composition/输入源切换，不当逐引擎IME补验。 QA92在dd9/0.6当前Mac模型显示名称用青简输入法，用户报告输入你好、全选替换、Esc取消、候选状态Enter只落字且弹窗保留，底部取消正常；取消后原JPEG/AX显示无壳/遮罩、原模型长名，Core配置和12条请求成本退出后保持。无候选/Enter原帧，人工报告单列，不自动切源。 2026-11-01用户明确“不用测试另外一种输入法了，跳过”：取消当前Mac第二种中文输入法交叉验证要求，保留已有青简结果；这是用户范围决定，不是另一输入法测试通过。 QA93复用855/f756 UI与dd9/b920 Core，用户报告供应商名称/掩码Key/API地址整组及上游模型ID的青简候选Esc/Enter与取消正常；原图只独立确认取消后原值，不虚构逐字段候选。用户出门后授权代理继续：CUA真实逐键搜索与最大输出tokens代表，预编辑ni'hao经Esc消失；数字原图见下划线，Enter实际落ASCII nihao且modal保留，取消后原模型/ID/已保存恢复，不称中文候选落字通过。唯一save403保留，用户补充点过测试连接，与test()先save路径相符；后续无新增变更请求，冷读config/models字节及5业务表全字段相同、2请求/成本保持。
- 剩余工作：当前Mac另一种中文输入法按用户要求跳过，不再要求切换。当前青简显示名称、供应商三字段、上游ID以及搜索/数字代表已有分层证据，不重复；先按源码合并其余不同提交/关闭处理的特殊路径，再仅补实际缺口，不能将代表或ASCII Enter扩称所有字段中文候选提交通过。跨平台原生输入仍由R02核对，安全Key不从AX猜值；禁止自动切源/开启AT。
- 验收标准：在所用输入法下，合成未提交时不误保存/关闭；字符串往返不损坏；不自动切换输入源，CUA typeText不冒充IME。当前Mac另一种中文输入法交叉验证按用户要求跳过，不计为测试通过。
- 历史记录：UI-46、UI-50、UI-51、UI-75、UI-76、UI-81、UI-92、UI-93。

### R01.4 辅助功能与七主题/三语言

- 已完成/已有证据：修复请求/汇总表Table/Row/ColumnHeader/Cell名称、完整值和索引；真实CUA树及两尺寸原图已查。七主题×三语言21组合均经真实交互和业务不变断言；7张中文主题代表原图已审阅。 QA77四类SelectItem和供应商自有协议/预设/思考菜单具名，原生AX选择、键盘确认/取消通过；模型行补完整名称/ID/选择文字，保持Table/Row/Cell及原selected；模型/协议/删除弹层具名Dialog，源码modal标记。 QA86采用Kit0.7.3可选Fast0.1.7，实际锁定crate校验后确认节点/焦点关系/动作监听回放；cached Window参数以retained-gpui::Window作编译期后端约束。当前原生AX和语言/主题代表路径已查；未开启真实AT，不签整个无障碍门禁。
- 剩余工作：真实辅助技术主路径仍未通过，严格禁止自动开启VoiceOver/系统辅助功能或切输入源。macOS adapter不将Row视为item-like，原生selected仍受限，行文字仅可读补充；菜单AX selected随cursor移动，不能等同committed checked。CUA不显示modal标记，关闭控件为应用Dialog body兄弟，完整AT作用域/焦点路径未验；不重复21主题组合。
- 验收标准：无无名关键控件，真实辅助技术能完成主路径；所有皮肤/语言可读且无截断，不要求重做280张相同截图。
- 历史记录：UI-34、UI-38、UI-40、UI-41、UI-57、UI-75、UI-77、UI-86。

### R01.5 40帧界面集中签收

- 已完成/已有证据：R01集中签收：官方51:2仍40界面，已逐一实际查看40份新参考图和80份原批次两尺寸PNG；格式/像素/hash/原候选来源全核，40帧无新增布局阻断。实际数据/窄窗滚动/原生AA差异逐帧记明；历史图未重标新R01候选。
- 剩余工作：40帧界面集中签收已完成；共享状态、完整键盘/IME/真实辅助技术仍分别R01.1–R01.4，平台/性能/发行仍R02–R04。Issue3品牌收尾独立提交，不重设计页面。
- 验收标准：每帧布局/资产/文字角色/状态/交互有最新设计证据；原生AA差异记录但不设RGB/字体文件字节相同门禁；无缺图/旧候选冒用。
- 历史记录：UI-53、UI-55、UI-56、UI-57、UI-58、UI-62、UI-63、UI-64、UI-65、UI-66、UI-67、UI-68、UI-69、UI-70、UI-71、UI-72、UI-73。

## R02：Windows、macOS与HiDPI

覆盖画板：全局发布门禁，不新增画板计数。

已有成果：QA78补同机原生窗口/重开、Windows标题栏避让和三平台CI包证据；Windows GDI首次受CI屏幕范围阻塞，完整输入/缩放/系统行为仍待。

本批交付：完成发行平台真实运行；类型检查和Rosetta不替代原生平台验收。

### R02.1 Windows原生窗口与输入

- 已完成/已有证据：已识别工具栏与系统窗控可能重叠；GNU cross-check受windres资源工具限制，不能标通过。 QA78确认Windows标题栏24px与工具栏top8的真实源码重叠，已改top32且增加几何回归。74b的Windows原生build/207test/集成与包生命周期通过，GDI因DPI96工作区放不下1440×900而失败、0截图，随后harness退出0并清理；2da改进临时CI显示准备，原生截图仍待新run，不冒称平台通过。 QA78补充：2da Windows job已终态success，实际GDI两尺寸原PNG已逐图审查，caption顶行与业务工具栏下排无可见重叠；DPI96，实际client1440×900/1100×700，原生边框PNG1442×901/1102×701。仅默认英文空请求页，未点击caption窗控或验证Ctrl/IME。 QA80当前957 Windows原run成功；两GDI原PNG实际外框1442×901/1102×701（目标client1440×900/1100×700），父代理逐图复核工具栏与caption无可见重叠，默认英文fixture64模型/0请求，不代表真实caption点击/IME/缩放/托盘验收。 QA82真实Windows旧/新EXE在Unicode自选路径覆盖、host/Core/GPUI打开关闭重开与退出、注册/快捷方式卸载通过；没有发送键盘/鼠标或进行新像素/IME/缩放/托盘输入验证，R02.1仍pending。 QA87新Fast UI568/Windows cd1a266c与冻结957 Core880016c6复用原CI产物，独立无编译run41272932710首轮43秒通过；两原GDI PNG1442×901/1102×701逐图/hash审查，客户区1440×900/1100×700、DPI96，caption和toolbar无可见重叠，小窗筛选换行。默认英文1供应商/64模型/0请求，未实际点击caption/托盘或发送键盘；正常生命周期、草稿保留、退出和显示恢复通过，不签物理输入/IME/HiDPI。 QA92 UI855/Win215b15d6与Coredd9/ed1d6440复用冻结产物，唯一native run41290170711/sourcef9d3f58/attempt1通过；两原GDI PNG1442×901/1102×701逐图/hash核对，caption/toolbar分离、侧栏顶部对齐、小窗筛选换行；默认英文1供应商64模型0请求，非物理caption/IME/HiDPI。显示1920×1080恢复1024×768验证成功，两个远端自有fixture合计205,997,568B清理为0。
- 剩余工作：已取得Windows100%两尺寸GDI原图和视觉分离证明；继续原生caption最小化/最大化/关闭实际点击、标题拖动、Ctrl快捷键、中文IME、托盘及代表弹窗。
- 验收标准：原生Windows可操作；工具栏不挡最小化/最大化/关闭；优先GDI/ImageGrab按PID截图，不能用Mac图替代。
- 历史记录：UI-43、UI-78、UI-80、UI-82、UI-87、UI-92。

### R02.2 macOS arm64系统行为

- 已完成/已有证据：当前arm64原生两尺寸/部分窗口与生命周期已验。 QA78同4905原生UI观察全屏动作、zoom、角拖动和toolbar恢复1440；三次设置窗口原生关闭重开，Core同PID。最小化实际状态/Dock恢复、物理托盘/登录/多屏仍缺，快捷键退全屏未证实。 QA81同一signed测试wrapper冷重开后一次普通窗口角拖获得1100×701，数据设置和请求页可滚动阅读；不是表头列drag、minimize/Dock或真实发行签名补验。 QA92用户实际黄色最小化/Dock恢复及绿色全屏/退出后原页/端口56290/12请求保持；CUA也进入真全屏。用户揭示顶部浅条，原1920×1080红图直接复现，源85540e1将背景移出Kit fullscreenpl_3的内部dragbar后，普通1440×901/全屏1920×1080/退出后1440×901原图对齐、浅条消失，用户最终确认两状态已齐。Core同59297未重启、业务/配置保持。工具一次CtrlCmdF未确认退出按失败保留，不否定用户实际退出成功。
- 剩余工作：QA92最小化/Dock恢复与全屏往返及背景修复已补当前Mac子路径；仍需物理托盘、实际系统登录/开机启动、多屏拖动、发行环境真实WorkBuddy读取配置与现有会话互操作。没有自动改系统设置；隔离loopback或Cocoa目标不能替代真实会话。
- 验收标准：实际状态有原生证据；后台代理续存；不强制置顶；系统登录等未做的场景明确保留。
- 历史记录：UI-35、UI-36、UI-42、UI-78、UI-81、UI-92。

### R02.3 Intel原生发行包与硬件

- 已完成/已有证据：x86_64编译/链接、thin测试及Rosetta真实运行已完成。 QA78 b167在macos-15-intel原生x86_64 runner实际构建并完成DMG包启动/关闭/重开/正常退出，包报告和artifact digest均保留；尚缺原生截图/完整主流程、托盘及匹配性能，不以Rosetta或PID记录签收整项。 QA79核对2da Intel原run124627179928最终success及artifact13054867223 digest，仍只是包/现有harness证据。771 Intel Corecredit测试失败保留，7bec原run继续核验。 QA82macos-15-intel原runner实际旧DMG应用替换/架构版本签名/新Core数据迁移/原生host与GPUI生命周期/卸载保持数据通过，未作用户物理设备/系统输入场景签收。 QA90只读环境核对为ARM Mac16,10，arch-x86_64 sysctl.proc_translated返回1；仅证明Rosetta可用，未运行translated GUI、不当Intel原生GPU/窗口/托盘/性能验收。
- 剩余工作：使用Intel硬件或可验证原生执行环境跑安装/启动/主流程/窗口/托盘/性能；缺环境则阻塞本门禁。 原生Intel完整GUI/托盘/匹配性能仍缺；不能凭CI构建或生命周期报告签收。
- 验收标准：产物架构正确，原生GPU/窗口运行通过；Rosetta证据单列，不冒称Intel实机。
- 历史记录：UI-52、UI-78、UI-79、UI-82、UI-90。

### R02.4 缩放、双尺寸与平台回归

- 已完成/已有证据：macOS已有1×/2×不同轮证据；并非最终全平台缩放矩阵。 Issue3侧栏品牌改用现有128px资源，macOS1x前后两尺寸通过，200%与Windows运行未验。 QA78Windows按钮几何按1100/1440和1/1.25/1.5/2逻辑投影回归；只是源代码矩形分离，非真实系统缩放矩阵。当前Mac仍1920×1080单1x显示器，Windows新原图待CI。 2da Windows100%默认英文代表页两尺寸原图已复核，品牌无可见白方块；其余页面/主题/长文本/弹窗与125/150/200%仍待，不签收完整矩阵。
- 剩余工作：Mac1×/2×、Windows100/125/150/200%覆盖代表性页面和长文本/弹窗，平台修复后回查共享控件。
- 验收标准：1100×700和1440×900逻辑尺寸下按钮/字段/尾部可达；系统额外截图行如实保留；平台改动不破坏Mac定位。
- 历史记录：UI-43、UI-52、UI-57、UI-74、UI-78。

## R03：最终性能与Core兼容回归

覆盖画板：全局发布门禁，不新增画板计数。

已有成果：QA80最终同Core原生P/C/C/P与Core全套契约通过，R03.1/R03.4已签收；启动慢样本、4096焦点CPU与长时驻留保留，R03.2/.3/.5继续pending。

本批交付：同一最终候选完成累计基线对照与核心全套契约，不用一次请求benchmark代替UI性能。

### R03.1 统一最终基线和测量条件

- 已完成/已有证据：QA80已冻结9570451 UI4658与a6e UIba525，双方同Core13c/1146；保留原1000预种子/20barrier/64模型/1000小+80×1MiB@4、同原生序列/1440×900/10秒dwell。原current1多一次语言菜单开合，原图未覆盖，前三轮保留诊断；在P2开始前预定并完成连续P2/C3/C4/P3主样本，不按数值挑样。全部7份JSON/JPEG/AX/独立RSS/峰值/livevmmap/体积/脚本SHA齐全，所有driver0/Core0/mock-15且UIgone。最终同Core1146/UI4658对a6eUIba525主P2/C3/C4/P3各n2：UI空闲中位差+0.000MiB、合计-0.515MiB、峰值-1.865MiB、进程→记录+209.764ms、proxy p95-0.0035ms。启动marker非首帧/可交互，proxy延迟非FPS。QA79完整旧25f基线阶段另列，不与最终同CoreUI对照混用。
- 剩余工作：
- 验收标准：记录Core/UI/合计RSS、二进制体积、启动、p50/p95及原负载；不混候选/架构，不丢慢样本。
- 历史记录：UI-2、UI-52、UI-57、UI-66、UI-67、UI-78、UI-79、UI-80。

### R03.2 峰值差异诊断与最终结论

- 已完成/已有证据：UI67匹配后空闲合计+0.58MiB，峰值+12.63MiB，尚不能宣称完全无回退。 QA78发现模型4096弹窗600秒真实采样，UI首末分钟中位RSS124.328→126.594MiB，+2.266MiB，单核平均CPU6.656%；保留增量与全样本，待普通菜单对照及最终累计基线，不宣称无增长或性能通过。 另语言菜单600秒首末分钟中位-0.109MiB、单核CPU1.605%；不同进程/操作历史，不能按匹配A/B归因。 QA79中间完整基线四轮合计空闲-0.07MiB/UI+0.14MiB/峰值+1.815MiB；f807并非最终Core，历史成本不撤销。livevmmap发现current1图形驻留；另C3立即125.6M/graphics62.6M降至15/30秒70.8M/10.1M，保留全部样本，不解释QA78长时增长。最终Coreheadless就绪468/52ms对基线52/52ms，慢样本原因未定。 QA80 最终同Core1146/UI4658对a6eUIba525主P2/C3/C4/P3各n2：UI空闲中位差+0.000MiB、合计-0.515MiB、峰值-1.865MiB、进程→记录+209.764ms、proxy p95-0.0035ms。新UI启动marker1562.402/779.425ms对旧1103.449/818.850ms，全部慢样本保留，未重跑到理想数值。4096最终900秒RSS+1.25MiB，footprint144.8→145.6M，allocated27.3→27.3M、objects78043→77995、fragmentation22.2→23.0M；仅解释该观察中驻留/碎片组成，不作普遍无泄漏结论。 QA84 closes the missing focus-routing evidence: preregistered sameprocess A1/B1/A2/B2 90s each at64/256/1014/4096, actualkey editing vs Space invert0to4096to0, CPU6.455/1.522/6.355/1.478% singlecore. Frozen4658 section-identical symboltwin resolves realA BlinkCursor/Proxy render/Taffy paths absent inB. No runtimefix, startupslowmarker and finalperformance verdict remain. QA85 WindowContent broadcache trial82f waswithdrawn after sourceinspection foundcachedreplay lacks accessibilitynodes/actions whilea11ybuilder clears perframe; transientAXempty/stale observations retained, uniquecausalitynotclaimed. FirstP1 lowCPU2.089% lackscontinuousactivity metadata; laterpointinactive, no validcandidatecomparison orCPUfixclaim. Runtime source stays957/UI4658. QA86 5681103/UI3c19对旧4658、同Core56285/1146、64/256/1015/4096/q前缀/选0，一次预定后台BGP1/BGC1各90.005秒、19点全inactive且前台同ChatGPT宿主，实际输入路由前后保持。UI单核6.6996→2.8221%，约降57.88%；Core均0.1555%。n1固定顺序/进程历史及遮挡未测，不作前台/峰值/整体R03签收；原P1前台条件t0失败及C1计时前取消原件保留。 QA88当前3c19结束900秒后台样本后vmmap原报告：UI footprint129.9M/系统生命周期peak130.0M，Core15.2M/16.1M；不是计时区间peak或匹配基线，RSS与footprint分开。 QA89发现4组纯契约断言在main解析--version前每次执行；b617将原body逐字节迁到cfg(test)并限定仅契约使用的兼容API/存储，未改实际渲染/业务handler/Core。新UI3c0e本机/三平台UI-only及后台原生启动关闭通过，但没有历史分阶段trace，不能将1562.402ms唯一归因于这4组断言，也不把CLI版本时延当首帧。
- 剩余工作：新候选后台CPU一对样本已改善，前台条件仍未建立；启动慢marker、峰值/footprint及最终性能结论待验。保持500ms光标/原负载，不将后台结果替代前台门禁。
- 验收标准：保留首次异常和直连对照；RSS与footprint不互相替代；不靠放宽负载/门槛通过，最终结论解释峰值成本。
- 历史记录：UI-67、UI-78、UI-79、UI-80、UI-84、UI-85、UI-86、UI-88、UI-89。

### R03.3 长列表、重开与静置观察

- 已完成/已有证据：4096候选/256模型和64供应商路径已有；部分菜单30秒CPU短测有证据。 QA78同4905真实64供应商尾项/255模型末项/1014请求102页/4096发现候选滚动搜索均可达；未添加或保存。发现弹窗600秒与三次原生重开已归档，语言菜单另600秒已完成，原图/样本/终态均归档。仍需按最终匹配数据解释增长并汇总剩余长时证据。 QA79短负载图形驻留追加15/30秒观察已归档；仍不能代替QA78发现弹窗600秒+2.266MiB的持续增长判断。 QA80同Core1146依次4905/6d93/4658三个900秒实际4096观察：CPU6.853/6.514/6.707%单核，RSS首末分钟中位差−4.984/+3.688/+1.250MiB，进程/操作历史和OS状态并非严格A/B。最终同进程一次Tab后300秒CPU1.693%、RSS−.063MiB，但AX/截图不能确立具体焦点，不断言光标根因。最终4658功能与冷重开两进程通过；全部五个长fixture UI与Core/driver均退出。 QA84 sameprocess keyboard-proven focus alternation replicates meanCPU6.405% search vs1.500% nontext. Ten original1440x900JPEGs, rawps samples/4rawprofiles and code-identical symbolmaps retained; config and1014 request/cost fields equal through shutdown. 90s is diagnostic, not900s longresidence/leakfree proof; highCPU remains unfixed. QA85 narrowsnextstep toA11ysafe rendering/cache boundary; no C1/C2/P2 or900s repeatsafteruserstreamlining. Trialfunctional input/selection/auto1014to1015refresh/size/theme/language/page passed, old1014fields+new1015exactafterexit, buttrialnotdelivered andnotfinalperformanceacceptance. QA86同一后台对照UI RSS旧123.125→123.109MiB、新116.391→116.641MiB；Core两段开始RSS22.594/25.188MiB。不是footprint/峰值/泄漏或长期驻留结论；本轮未重复900秒。 QA88实际当前3c19/Core1146，64供应商/256模型/1014真实请求成本/4096候选/搜索前缀/选0；10秒warmup后900.010秒61点全inactive、同前台96217且PIDbirth逐点一致。UI CPU单核2.7244%、首尾分钟RSS中位112.867→109.406MiB，差−3.461MiB；Core CPU.1544%。前后实际z/Backspace路由，Cancel/close通过，配置字节及1014请求/成本全字段在退出后保持；Core/driver0、UI仅PIDgone。n1后台无持续RSS增长，不作无泄漏/前台/交互延迟或总体性能签收。
- 剩余工作：Fast缓存/无障碍回放已交付，当前90秒后台对照及900秒后台驻留已补证；仍需有效、可比的前台活动窗口条件及最终长期响应性结论。后台RSS下降不能证明无泄漏或替代前台CPU/footprint/峰值与整项验收；不重复相同后台样本。
- 验收标准：无可复现持续内存增长、无空闲异常高CPU、交互无明显阻塞；请求延迟不冒充FPS或输入时延。
- 历史记录：UI-20、UI-29、UI-37、UI-41、UI-51、UI-58、UI-78、UI-79、UI-80、UI-84、UI-85、UI-86、UI-88。

### R03.4 Core全套契约与数据不变

- 已完成/已有证据：QA80 13c修复启动前SQLite Busy/Locked使stats永久disabled：迁移态250ms可中断重试，永久/schema-too-new仍拒绝；recover/start_run/interval同事务原子提交。三项确定性红绿回归及真实启动前EXCLUSIVE7秒→释放后同PID ready/import1/12tokens/.1credit/两run各1interval通过。Core580本机测试/27命令/91源码hash、158导入/15故障/19发现/10创建及两既有GPUIignore实际Core补验通过；957最终Core+三平台CI均首次通过。真实longfixture1014请求与成本逐字段一致；原配置所有字段保留，四次允许language写序列化仅补64protocol+64settings+256capsnull共384既有默认字段，配置字节/原始结构并不相等。74b/771/7bec原始失败、无诊断限制、invalid首个probefixture均保留；不声称唯一归因历史CI。 QA81补真实已发布v056/schema6与v057/schema11包的Core→当前957CI Core冷交接：实际旧Core生成12条有价历史、手工价、731天与OBSERVE，schema6→11及11→11逐旧字段/配置字节保持，当前Core各新录第13条。小窗前后全13逐字段一致，原12对照旧版备份跨两native仍一致。此为macOSarm64新增证据，运行时代码未改。 QA82两真实旧版×三原生runner实际安装替换后12旧行/价格/非默认设置/Key配置保持，新13行计价且已装UI生命周期后不变。属于新增包级数据证据，未改运行时代码。 QA83 adds real package/full-profile cold rollback evidence on3platforms x2oldversions with frozen957 runtime. No new runtime build/full Core matrix; original scope retained. QA91冻结dd9/0.6完整Core/三平台矩阵及六例真实旧包冷升级回滚通过；Linux579/桌面580Core，158导入/平台及Linux15故障均独立报告核对。新WAL修复仅测试快照读取，不改变Core/DBschema。
- 剩余工作：
- 验收标准：原配置和Key兼容，取消/失败/保存边界不回退，历史快照/导入去重不破坏；全部结果绑定最终提交。
- 历史记录：UI-22、UI-24、UI-33、UI-42、UI-46、UI-62、UI-63、UI-65、UI-67、UI-78、UI-79、UI-80、UI-81、UI-82、UI-83、UI-91。

### R03.5 最终构建和性能报告

- 已完成/已有证据：GPUI最新206通过/2既有忽略，fmt/strict all-targets Clippy/Intel all-targets check/arm64release已有；最终发行提交统一门禁仍待执行。 QA78 GPUI208pass/2既有ignore、fmt/strictClippy/Intelcheck/arm64release通过，Mac UI仍4905/Core25f；Win项目平台差异207pass/2ignore。原始fixture两ignored通过单列；同源码最终CI/平台/性能均按实际候选分别绑定，未虚假全绿。 QA79最终Core7fa463/当前UI4905已冻结；24项本地检查通过、Core577pass，两个GPUI既有ignored在真实Core条件补验通过。新CoreCI集成锁恢复未过，最终native性能无样本，不能签收最终报告。 QA80运行时代码957/UI4658+Core13c/1146已冻结，Core580/GPUI213pass+2既有ignore、27+6本地命令/strictClippy/Intelcheck/release及最终四CIjob全部成功；QA80完整原始报告/当前7轮对照/长时CPU与heap/首次失败/清理归档。 QA84 diagnostic-only unstrippedbuild0, all23filebacked section bytes/addresses equal, shared frozen flatoutput restored. No productsource/registry edits or newCI. New focus/symbol evidence does not sign overall performance. QA85 trialfmt/213unit/strictclippy/Intelcheck/ARMrelease passed,2existingignoredrealCorecontracts separately passed19discovery/10creation. OnlyCI/README changes delivered3e257fd; trialsource revertedbyteequal andshared4658outputrestored. NewUI-onlyCI isscoped, notnewCore/fullRCmatrix. QA85精简入口首次运行41262088104绑定3e257fd，Windows/ARM/Intel三UI job全部success，完整Desktop/旧升级skipped；原日志与产物元数据归档，UI artifact未下载，不代替最终全矩阵或硬件/安装验收。 QA86 5681103/UI3c19/Core1146：本机fmt、213pass/2既有ignore、strictClippy、Intelcheck、ARMrelease及CLI通过；真实Core契约沿用未变业务源码的QA85补验，不冒称本轮重跑。首次Fast32beb空壳失败和静态后端约束crate别名编译错误均保留；修复3c19已原生红绿验证。新的三平台UI-only阶段CI按dispatch.json核对，最终全RC矩阵仍需集中执行。 QA86阶段UI-only run41270728784绑定5681103，Win/ARM/Intel三job首轮success，Win212/2ignore、两Mac213/2ignore；完整Desktop/旧升级skipped，原log/产物元数据归档，未下载ZIP，不替代最终全RC矩阵。 QA89当前b617/UI3c0eb8c7/Core1146：fmt、strictClippy、216pass/2既有ignore、Intelcheck、ARMrelease/CLI通过；唯一UI-only run41275489271三平台首次success，Win215/2ignore、两Mac216/2ignore。新原生仅2供应商/5模型/1实际请求启动与三页可用/正常关闭，配置字节与1请求成本保持；旧3c19后台性能不重标新UI，最终性能仍pending。 QA91新0.6冻结dd9的LinuxCore与三平台完整Desktop首轮通过，Windows215/两Mac216UI且各2既有ignore；实际包/回滚原始报告齐备。没有再跑前台/启动/峰值或后台90/900秒，整体性能门禁仍pending。 QA92仅UI绘制修复855/f756，fmt/本机ARMRelease/CLI0.6与唯一三平台UI-only41288999354首轮成功（Win215、两Mac216通过，各2既有ignore）；真实全屏红绿和用户确认/数据保护归档。Windowsnative41290170711精简复核通过，没有新Core/全矩阵/升级或性能样本；最终性能及最终RC仍待。
- 剩余工作：构建与报告已齐，整体性能结论仍受R03.2/R03.3待验限制；后续若改运行时代码须重新绑定相应最终验证。
- 验收标准：既有忽略说明/所需真实Core fixture补跑单列；测试/链接/硬件/GUI证据范围分明，无虚假全绿。
- 历史记录：UI-52、UI-67、UI-78、UI-79、UI-80、UI-84、UI-85、UI-86、UI-89、UI-91、UI-92。

## R04：RC安装包到正式发布准备

覆盖画板：全局发布门禁，不新增画板计数。

已有成果：QA80最终957四CIjob/4artifact已核验；QA82真实3平台×2旧版本安装覆盖升级、已装UI生命周期与卸载保持数据6例首轮通过，R04.1/.2签收。签名信任/公证、最终版本说明回滚与公开发布R04.3/.4/.5仍pending。

本批交付：产出可审查RC与完整发布记录；所有门禁通过后才具备正式发布条件。

### R04.1 最终提交CI全矩阵

- 已完成/已有证据：QA80最终9570451 Core41246370582、Desktop41246381635（arm124670578779/Intel124670578891/Windows124670578931）四job实际completed/success。Core158导入/15故障、三个包报告与4ZIP size/SHA核对，原始JSON与两Windows GDI PNG逐图保留，source均957。较早13c及b2也各四job成功，7bec Desktop现全绿但其Core156/2首次失败仍保留。一次SSL push失败后误dispatch了旧13c两重复run41244232391/41244235522，核实实际head后取消自有重复run，正常push/remote校验后才正确dispatch b2/957；未绕过TLS或重跑失败到绿。 QA81只新增独立真实旧版数据验证脚本/报告，运行时代码仍957；未新派发CI，不把本机arm64升级测试说成新全矩阵通过。 QA82新增只运行升级的手动分支：0a355测试源码验证冻结957包；原Desktop全构建job故意skipped，3个升级job首轮success，不冒称新一次Core全矩阵。完整运行时代码仍957，原四job证据保持。 QA83 adds real package/full-profile cold rollback evidence on3platforms x2oldversions with frozen957 runtime. No new runtime build/full Core matrix; original scope retained. QA85 adds ui_only dispatch forGPUIchecks onWin/ARM/Intel withoutCore/installer/migration. Defaultfullworkflow preserved; runtime owns957unchanged, priorfullmatrixsameowningsource evidence retained. ScopedUIrun41262088104 isnotnewfinalCore/fullRCacceptance. QA85精简入口首次运行41262088104绑定3e257fd，Windows/ARM/Intel三UI job全部success，完整Desktop/旧升级skipped；原日志与产物元数据归档，UI artifact未下载，不代替最终全矩阵或硬件/安装验收。 QA86运行时UI实质变更为5681103/3c19，Core1146未变；因此本项按新候选重新pending，旧957完整矩阵原证据不撤销也不重标。新UI-only阶段检查不替代最终Core/三平台/打包全矩阵。 QA86阶段UI-only run41270728784绑定5681103，Win/ARM/Intel三job首轮success，Win212/2ignore、两Mac213/2ignore；完整Desktop/旧升级skipped，原log/产物元数据归档，未下载ZIP，不替代最终全RC矩阵。 QA87新增windows_native_only入口复用已通过的UI568与Core957固定产物，校验source/run/归档大小SHA后只跑两尺寸GDI及既有生命周期；唯一run41272932710 attempt1成功，完整Desktop/UI构建/旧升级三job skipped。非最终全矩阵/正式包，R04.1继续pending。 QA89 runtimeb617/UI3c0e纯契约移出生产入口；唯一run41275489271 attempt1三平台UI-only首轮成功（ARM124756717668/Intel124756717805/Win124756717921），完整Desktop/旧升级/Windowsnative三job skipped。原日志/三artifact服务器元数据保留，未下载ZIP、不作全RC或硬件验收；未来Windowsnative固定UI来源同步到13071258074，Core冻结不变，未重跑GDI。 QA91运行/打包冻结dd9deba、0.6.0：唯一Core41278921019和Desktop41278929532 attempt1正式success；Linux579、桌面各580Core，Win215/两Mac216UI且各2既有ignore。三平台fmt/strictClippy/Release/集成/包生命周期、Windows安装器/GDI均通过，Core158导入/15故障与三Desktop各158导入原JSON独立下载校验；4ZIP（Core+3Desktop）来源/大小/SHA核验，四个候选包及校验清单保留本地审查，未公开发布。 QA92人工全屏发现顶部侧栏背景错位，已原生复现并修复为窗口锚定背景；运行UI变化为85540e1/f756a709，Coreb920/0.6未改。故最终全矩阵按新候选重新pending；QA91 dd9完整四job原证据保留来源，阶段UI-only不替代最终矩阵。 QA92阶段UI-only原run41288999354绑定855/attempt1，ARM124796131735、Win124796131859、Intel124796131982全部success，原log/三artifact元数据核对，未下载三UI ZIP本机。native-only41290170711绑定f9d/attempt1复用新WinUI855和旧dd9Core通过，独立报告ZIP digest/原图/清理核验；完整Desktop/Core/升级故意skipped，不签当前最终全矩阵。
- 剩余工作：当前85540e1 UI背景修复需阶段三平台UI检查，并在最终实际打包候选集中绑定完整RC；QA91 dd9全矩阵不重标新UI。无需在纯绘制阶段重复Core/导入/六例升级。
- 验收标准：Core+三平台工作流实际终态通过；待排队/无run/仅本地检查均不标绿；所有报告与候选SHA一致。
- 历史记录：UI-78、UI-79、UI-80、UI-81、UI-82、UI-83、UI-85、UI-86、UI-87、UI-89、UI-91、UI-92。

### R04.2 DMG/ZIP/EXE安装升级卸载

- 已完成/已有证据：QA82真实三平台旧包安装/覆盖升级/卸载签收：测试源码0a355、运行时仍957；官方v056(schema6)/v057(schema11)先通过真实旧Core写12有价行/手工价/731天/OBSERVE/Unicode与配置Key，再关闭旧Core、实际Windows旧setup→候选setup同一Unicode自选目录覆盖或Mac只读DMG app同路径整体替换（不是Finder拖动）。安装本身保持旧DBschema/业务字段/配置字节，候选schema11新录第13有价行；已安装host/Core/GPUI真实打开、关闭留同Core、重开新UI、正常退出，之后全13业务字段与配置字节仍同。Windows包全部portable文件SHA/版本/x64/注册/快捷方式正确；卸载移除文件/注册/快捷方式且不启用登录启动，外部数据保持。Mac所有资源/链接一致、两bundle strict签名和架构/版本正确，移除已装app而外部数据保持。原run41254168866 Intel124693699432/ARM124693699544/Windows124693699610均首轮success；6例原报告与3ZIP digest核验，三remotefixture清理0。直接CLI Core0/waited，nativehost0，host拥有的Core/UI退出码未知但OSPIDgone与3normal run已证。限冷升级/隔离自选外部profile，不当热更新、FinderTrash/Gatekeeper/SmartScreen/像素键盘/物理设备验收；回滚与最终分发仍R04.4/.5。旧sameversion stub/lifecycle及QA81冷交接按原scope保留，不替代本轮真实安装步骤。 QA83 adds real package/full-profile cold rollback evidence on3platforms x2oldversions with frozen957 runtime. No new runtime build/full Core matrix; original scope retained. 0.6.0新候选版本准备中，旧957/0.5.7安装升级回滚证据保留原来源；新候选产物/版本/安装与旧版替换尚需绑定验证，暂重新pending。 QA91正式0.6候选包绑定dd9/fullDesktop41278929532，测试21dea的首run41281534447为Win/ARM成功、Intel v056在Core正常退出0后被非空WAL冷读断言拒绝，未到GUI/回滚，原报告与失败保留。确定性用例证明PASSIVE全部checkpoint后WAL仍可非空；修正测试冷快照只在一次性DB+WAL副本读取完整提交数据，原DB/WAL字节不变、活动标记守卫和13行/所有业务字段断言保持。新测试源c435b5c的唯一run41282248853 attempt1三平台首轮成功，3WAL回归/平台与v056/v057六例实际安装升级、GUI生命周期、旧包/完整profile冷回滚、卸载均通过，全部artifact独立SHA/size及脚本LF/精确WindowsCRLF身份核对，原包等于保留交付物；3runner专用fixture和额外快照副本全部清理。
- 剩余工作：本候选0.6.0的实际六例冷安装升级/原包及完整目录回滚已签收；限隔离profile，不含热降级、历史合并、旧GUI、Finder拖放、真实WorkBuddy会话或下载信任行为；后者仍按R02/R04.3/.4/.5保留。
- 验收标准：包内Core/UI架构/版本/资源一致；旧配置/Key/用量数据按既有约定保留；无残留在用进程。
- 历史记录：UI-78、UI-79、UI-80、UI-81、UI-82、UI-83、UI-91。

### R04.3 签名、公证与可运行性

- 已完成/已有证据：目前不能把本地ad-hoc测试签名视为正式发行签名/公证通过。 QA78核对README现行分发为Mac ad-hoc未公证/Windows无Authenticode，源脚本与声明一致。本机只读查到Developer ID Application0；未读取/导出私钥或尝试签名上传。实际下载隔离属性/Gatekeeper/SmartScreen完整行为仍未验，不能凭包进程记录签收。 QA90只读find-identity核对本机DeveloperIDApplication仍0，未输出身份/导出key/解锁或上传。README现有Mac ad-hoc未公证/Windows unsigned方案明确，证书0不单独作为该方案mandatory blocker；真实下载后的Gatekeeper/SmartScreen行为仍待。
- 剩余工作：按项目正式分发方案核对macOS签名/公证/Gatekeeper、Windows签名/SmartScreen与可用凭据；缺条件显式标记。
- 验收标准：从真实下载/挂载或解压的候选包启动；签名链/公证或项目明确采用的分发方式有可审查记录；不在日志暴露密钥。
- 历史记录：UI-78、UI-90。

### R04.4 版本、说明、截图与回滚包

- 已完成/已有证据：当前项目版本0.5.7和历史QA/截图/矩阵存在，不能据此推断新的正式版本号。 QA78以b167/74b/2da分别保留源码、原始截图、候选/包/报告哈希与失败来源；尚未指定新正式版本、替换公开README截图或公开发布，回滚与最终下载包审查仍待。 另保留2da两张原始WindowsPNG的隐私/内容/hash审查，均合成fixture，未自动替换公开README截图。 QA83 test70712b8/runtime957: actual v056/v057 x Windows/ARM/Intel full cold backup and package restore passed6/6 in first CI run41256459894. Whole profile byte-equal before old launch; old schema6/11, prices/settings/config preserved; old Core prices a new request. Candidate13 data remains independently byte-equal, not merged. UPGRADE_ROLLBACK.md and README link added, original package URLs/SHA pinned. Two initial local RO-WAL harness failures retained; explicit connection close and guarded cold immutable reader validated, local ARM2/2 final PASS. No runtime/UI change or final release claim. QA91按已说明的默认0.6.0准备候选；版本清单统一、包标签拒绝漂移、CHANGELOG草案、回滚指南随包及Fast许可说明已落实。实际WinZIP/setup与两MacDMG共140,896,314B，四校验companion与独立SHA核对，原包保留于忽略的dist/studio-v2-rc/0.6.0，来源dd9/fullDesktop41278929532；原始Windows两尺寸0.6.0 GDI合成数据/隐私与caption分离已审查，未自动替换README图片。
- 剩余工作：0.6.0候选材料和四包校验已可审查；公开README代表图尚待最终替换/审查，正式版本与长期分发附件在最终出厂阶段确认。新版实际升级/回滚由R04.2单独核验，不能重标旧957证据；不将本地RC或临时CIartifact冒称正式公开下载。
- 验收标准：版本一致、可下载包SHA完整；公开图为合成隔离数据且无Key；文档命令可移植，回滚路径可执行。
- 历史记录：UI-78、UI-83、UI-91。

### R04.5 正式版出厂核对与发布

- 已完成/已有证据：当前Goal active，未制作新的正式Release；分支push不等于公开发布。
- 剩余工作：逐条核对P00、B01–B05、R01–R04；形成具体RC清单和最终结论，再执行明确授权的tag/Release发布动作。
- 验收标准：40帧界面签收＋所有发布门禁通过，无P0/P1、无未解释明显性能回退；安装/升级/回滚和证据齐备，才标可发布。
- 历史记录：按实际最终候选补证，不冒用历史通过。

## 完整功能保护项与验收任务映射

| 现有功能保护项 | 对应任务 |
| --- | --- |
| 七功能工具栏独立定位与遮罩层级 | B04.1 |
| Windows 标题栏与工具栏兼容性 | R02.1 |
| Intel 发行版与译运行 | R02.3 |
| 六套旧皮肤 + 可选 Studio 靛蓝、保存失败不切换 | B04.3、R01.4 |
| 开机启动、失败回滚 | B04.4、R02.2 |
| 检查更新、busy/error | B04.5 |
| 系统/中文/English | B04.2 |
| 恢复 1440×900、退出全屏 | B04.4、R02.2 |
| GitHub 与新 Issue | B04.5 |
| 最小窗口 1100×700、系统按钮/拖动 | B04.1、R02.4 |
| 供应商 0/1/2/20/64、固定 CRUD | B01.1 |
| 侧栏选中导航/供应商实色 | B01.1、B01.7 |
| 新增、复制、删除确认与取消 | B01.2、B01.4、B01.5 |
| 资料编辑/取消与历史草稿 | B01.3 |
| 新增/复制保存完成提示 | B01.2、B01.4 |
| 单模型提示/删除后结果 | B01.5 |
| 未保存切换/恢复草稿 | B01.3、B05.4 |
| 协议/预设/思考菜单、名称/URL、Key 显隐 | B01.6 |
| 四协议不同字段、认证、tokens/思考 | B02.4、B02.5 |
| 模型发现 4096、总量 256、手动编辑 | B02.1、B02.2 |
| 模型表键盘、焦点与语义结构 | B02.2、R01.2 |
| 模型行编辑图标与保存入口 | B02.3 |
| 模型表完整值提示 | B02.2 |
| 模型目录数量与配置来源标记 | B02.1、B02.2 |
| 模型能力自动/手动、思考档位与Token/上下文 | B02.3、B02.5 |
| Shift/Cmd/Ctrl 多选、快捷键、焦点/IME | B02.2、R01.2、R01.3 |
| Unicode 字段编辑、保存与冷载入 | B01.3、R01.3 |
| 模型编辑输入框 Escape | B02.3、R01.3 |
| 同步/取消、备份恢复与重启 | B05.1、B05.2 |
| 概览/明细/汇总/设置、全部/内置/自定义 | B03.1、B03.2、B03.3、B03.6 |
| 日期、分页、每页条数、固定表头 | B03.2 |
| 未知/缺失、缓存、Token 完整性 | B03.1、B03.3、B03.5 |
| CNY/USD/积分、价格快照与规则 CRUD | B03.4 |
| 采集、保留天数、本地导入去重 | B03.6 |
| 真实代理状态、计数、端口 | B05.2、B05.4 |
| 最近 100 条日志、清空/复制/滚动 | B05.3 |
| 最近请求、详情与独立读身份 | B03.1、B03.5 |
| 请求详情、元数据与本次单价 | B03.5 |
| 关闭 UI 释放、托盘/核心续存、单实例 | B05.4、R02.2 |
| 外观/中英/HiDPI/无障碍 | R01.4、R02.4 |

## 排程说明

依赖顺序排程，Figma恢复前不预填完成日期。

每批集中施工→一次冻结→一轮完整验收→一轮差异修正复验→提交；如发现阻断不跳过。

首次最新设计差异清单完成后再按批估时；Windows/Intel设备、签名凭据与系统登录场景的等待单独计，不把等待伪装成开发时长。
