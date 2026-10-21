> 最新价格口径：当前配置可随时修改/删除，逐请求保存单价与金额，缺价默认0。详见[PRICING-DECISION.md](PRICING-DECISION.md)。

# 统计管理接口契约 v2

本文为待实现接口。复用 `POST /_control/action`，不新增公网接口，不改聊天 API。

## 1. 传输、鉴权与版本

- 必须通过现有 loopback、Host/Origin 校验与 **management_key** 鉴权；local_key 无统计管理权限。
- 请求 Content-Type 为 application/json，沿用现有 128KiB body 上限；统计字段另做严格长度/枚举/数量校验。
- 保留外层协议：成功 HTTP200 `{"message":"<序列化后的统计JSON>"}`；错误 HTTP400 `{"message":"stats_invalid_argument: ..."}`。GUI Backend 先读取 message，再按统计 DTO 解析内部 JSON。
- 内层固定 `api_version:2`。每个统计成功响应有 `request_tag`（客户端传回值）、`generated_at_ms`、`generation`。
- generation 是 `<run_id>:<commit_revision>` 字符串，用于识别读取版本；commit_revision 是数据库工作线程每次成功提交后更新的进程内计数，不是请求数量。跨核心重启必须清空 GUI 游标与缓存。
- 原实现CONTROL_PROTOCOL已为5；本轮统计DTO变为v2，核查兼容后核心与GUI同步提升控制协议（预计5→6），核查 main/frontend/GPUI 对旧核心重启与版本匹配逻辑，保持现有生命周期行为；统计API尚未实现时不能假装成功返回空数据。
- 一次响应序列化后必须小于1MiB（与现有 GUI 读取限制一致）；超限报 `stats_result_too_large`，不能截断 JSON。
- 时间、数量、Token 使用 i64 范围内 JSON 整数；第一版单字段 Token 上限1e12。金额 nanos 使用十进制字符串，避免 GUI/将来JS客户端丢精度；数据库仍为 INTEGER。
- 非法输入、预算超时、服务忙分别使用固定前缀：`stats_invalid_argument`、`stats_query_timeout`、`stats_busy`。其他：`stats_unavailable`、`stats_cursor_expired`、`stats_revision_conflict`、`stats_not_found`、`stats_result_too_large`、`stats_query_too_large`（匹配超过100万条，请缩小范围）。
- 不回传 SQL、文件路径、鉴权信息或上游错误原文；错误提示用中英本地化映射。

## 2. 通用过滤器

```json
{
  "from_ms":1790784000000,
  "to_ms":1791993600000,
  "timezone":"Asia/Shanghai",
  "provider_id":null,
  "upstream_model":null,
  "route_id":null,
  "outcome":null,
  "usage_status":null,
  "record_kind":"all",
  "include_tests":false
}
```

- from/to 必填、非负，from < to，跨度<=366×86400000ms。时区必须是受 chrono-tz 识别的 IANA 名称，不接受SQL表达式。
- timezone 用于分桶和显示，不改变 UTC 时间过滤边界。GUI 按所选时区把本地日期转换为UTC，不能简单减固定8小时处理所有地区。
- ID/model 为精确匹配字符串，非空且<=160 Unicode标量，禁止控制字符；null代表全部，空字符串不等价全部。
- outcome/usage_status 取 schema.sql 的固定枚举。普通 UI 以单选为准，第一版不支持任意数组组合。
- `record_kind` 默认 `all`，可选 `model`、`tool`、`other_charge`。只把真实 WorkBuddy 导入记录中的独立收费项归为工具/其他收费；同名的普通代理模型仍属于模型用量。该条件统一作用于总览、序列、明细、汇总与游标校验，和内置/自定义来源相互独立。
- source 默认 relay；include_tests=true 时允许 relay+connection_test，不是只看测试。
- 所有统计接口共用同一个 validated `StatsFilter` → SQL predicate builder；不能为图表/表格分别手写不同默认条件。

## 3. stats_health

请求：`{"command":"stats_health","request_tag":"health-1"}`。

内层响应示例（字段完整，数值仅示意）：

```json
{
  "api_version":2,"request_tag":"health-1",
  "generated_at_ms":1791993600000,"generation":"run-a:17",
  "state":"ready","enabled":true,"schema_version":2,
  "collection_started_at_ms":1790784000000,
  "earliest_record_at_ms":1790784000123,
  "retention_days":180,"record_count":240,
  "db_bytes":409600,"wal_bytes":32768,
  "queue_depth":0,"queue_capacity":1024,
  "pending_requests":0,"dropped_requests_current_run":0,
  "write_failures_current_run":0,"last_commit_at_ms":1791993599100,
  "loss_scope":"no_loss_observed_in_current_run",
  "error_code":null,"settings_revision":1
}
```

- state=ready/disabled/degraded/migrating。schema损坏/打不开仍能返回health，不依赖可用DB才能报错。
- loss_scope 只取 `no_loss_observed_in_current_run` / `loss_observed` / `unknown_after_unclean_exit`。没有观测丢弃不等于历史零丢失。
- 当前进程内队列和错误原子计数即时返回，存盘可能滞后；不把这两份再相加。
- record_count/db_bytes 等昂贵数据缓存最长10秒，不在热路径stat文件或count全表。
- health 无须过滤器，GUI空闲不轮询；在统计页中最多每10秒一次。

## 4. stats_dashboard

请求：

```json
{"command":"stats_dashboard","request_tag":"dash-7","filter":{"from_ms":1790784000000,"to_ms":1791993600000,"timezone":"Asia/Shanghai","provider_id":null,"upstream_model":null,"route_id":null,"outcome":null,"usage_status":null,"include_tests":false},"granularity":"auto"}
```

granularity=auto/hour/day/week，最多400桶。一次只读事务中产生 summary 和 series，保证主卡与趋势来自同一数据库快照。

响应结构：

```json
{
  "api_version":2,"request_tag":"dash-7","generated_at_ms":1791993600000,
  "generation":"run-a:17","filter_key":"sha256-of-normalized-filter",
  "granularity":"day",
  "summary":{
    "request_count":10,"finished_count":9,"in_flight_count":1,
    "completed_count":6,"failed_count":1,"rejected_count":0,
    "cancelled_count":1,"interrupted_count":1,
    "usage_eligible_count":9,"usage_complete_count":7,
    "usage_partial_count":0,"usage_missing_count":2,"usage_invalid_count":0,
    "usage_not_applicable_count":0,
    "input_tokens":7000,"output_tokens":700,"total_tokens":7700,
    "cache_read_tokens":4000,"cache_read_known_count":5,
    "cache_read_denominator_input_tokens":5000,
    "cache_write_tokens":null,"cache_write_known_count":0,
    "input_uncached_tokens":null,"input_breakdown_known_count":0,
    "reasoning_tokens":100,"reasoning_known_count":2,
    "success_rate":0.6666666667,"usage_coverage":0.7777777778,
    "cache_hit_rate":0.8,"avg_duration_ms":1200.0,
    "costs":[{"currency":"USD","total_cost_nanos":"15400000","settled_request_count":7}],
    "default_zero_request_count":0,"cost_incomplete_request_count":2
  },
  "series":[
    {"start_ms":1790784000000,"end_ms":1790870400000,
     "request_count":1,"finished_count":1,"usage_complete_count":1,
     "input_tokens":1000,"output_tokens":100,"total_tokens":1100,
     "cache_read_tokens":800,"cache_write_tokens":null,
     "cache_read_known_count":1,"cache_write_known_count":0,
     "cache_read_denominator_input_tokens":1000,
     "costs":[{"currency":"USD","total_cost_nanos":"2200000","settled_request_count":1}],
     "quality":"known","collection_warning":false}
  ],
  "warnings":[{"code":"usage_missing","request_count":2}]
}
```

series 示例只列一桶；实际必须覆盖整个筛选范围。桶起止是UTC，首/尾可裁剪，不强求最后一桶满24h。

- summary Token 只合计 counted_usage=1 行。没有任何完整行：input/output/total均null。缓存等字段只有已知行才有合计，未知为null。
- series.request_count=0 时，在确认该时间段采集连续可用且无丢失证据的情况下，Token为0、quality=empty；否则Token=null、quality=unknown，collection_warning=true。历史停采/异常退出/丢失只可保守判定，不伪造完整覆盖。
- 有请求无完整usage：quality=unknown，Token=null；有完整但覆盖不全：quality=partial；全部已结束且应有usage的请求完整：quality=known。进行中另显示，不声称桶已结算。
- warnings 固定 code：usage_missing/usage_partial/usage_invalid/collection_disabled/collector_loss/unclean_exit/retention_limited/provider_observe_mode。附计数或时间范围，不拼接原始错误正文。
- 比率字段为0..1或null；同时给出分子/分母来源，前端不得从四舍五入后的卡片反算。
- failed_count包含failed与rejected，rejected_count是其子项；usage_complete_count用于Token主统计，覆盖率分子另限定upstream_started=true与eligible分母对齐。
- 每桶费用汇总本次已存金额（包含缺价默认0）；settled_request_count包含已确定0金额，default_zero_request_count说明默认来源；缺用量但有非零价的未知金额另列。

## 5. stats_requests

请求：`command,request_tag,filter,page_size,cursor,page?`；page_size=10/20/50/100，cursor初次为null。UI 可调整每页条数并保存本地偏好，可输入页码按回车或点击跳转。

内层返回：

```json
{
  "api_version":2,"request_tag":"rows-7","generated_at_ms":1791993600000,
  "generation":"run-a:17","filter_key":"...","as_of_id":240,
  "total_rows":240,"page_size":20,
  "items":[{
    "id":240,"request_id":"req-a","source":"relay",
    "provider_id":"p1","provider_name":"供应商A","route_id":"r1","route_name":"日常模型",
    "requested_model":"供应商A/model-x","upstream_model":"model-x","response_model":null,
    "started_at_ms":1791993590000,"ended_at_ms":1791993591200,
    "stream":true,"upstream_started":true,"http_status":200,
    "outcome":"completed","error_kind":null,
    "duration_ms":1200,"ttfb_ms":100,"first_content_ms":120,
    "usage_status":"complete","usage_reason":null,"usage_source":"upstream",
    "input_tokens":1000,"output_tokens":100,"total_tokens":1100,
    "cache_read_tokens":800,"cache_write_tokens":null,"reasoning_tokens":null,
    "input_uncached_tokens":null,"input_breakdown":"read_subset",
    "cost":{"status":"priced","currency":"USD","total_cost_nanos":"2200000","price_source":"manual","source_price_id":"price-a","source_price_revision":1,"price_snapshot_at_ms":1791993590000,"input_mode":"flat","input_rate_nanos":"2000000000","output_rate_nanos":"2000000000","cache_read_rate_nanos":"0","cache_write_rate_nanos":"0","pricing_version":2}
  }],
  "next_cursor":{"last_started_at_ms":1791993590000,"last_id":240,"as_of_id":240,"filter_key":"...","run_id":"run-a","maintenance_revision":0}
}
```

- 实际items为page_size行示例只列一行，next_cursor只在有更多行时返回，故服务端查询limit=page_size+1。
- 固定 `id<=as_of_id` 防止新请求插进翻页结果，顺序翻页按时间/id keyset；GUI 保留有界页游标记录以便返回。
- 可选 `page` 为从 1 开始的整数，用于页码直达；携带现有 cursor 时只复用其快照边界，重新定位目标页。请求明细先按索引扫描轻量时间/id 键定位，再读取该页详情，不逐页发请求。深页直达仍有与跳过键数量相关的成本，保留查询预算。
- 页码直达响应回显 `page`；旧核心若不支持并忽略此字段，GUI 会提示更新/重启，不把旧页误显示为目标页。非法、溢出或不存在的页返回 `stats_invalid_argument`，GUI 保留原页和数据。
- 游标不是历史MVCC快照：已有进行中请求仍会完成、本次历史价格和金额冻结、保留策略会删除旧行。总览与列表不同查询间可能有短暂变化，应显示各自更新时间。
- 核心重启、筛选改变、清理/保留删除导致maintenance_revision变化时，返回cursor_expired，GUI回第一页并说明“统计已更新”。
- 普通新增/请求完成不会使游标失效。含outcome/usage_status筛选时最终状态变化可能改变成员；刷新回第一页，翻页时可出现总数变化，不承诺冻结状态。需要严格冻结报表属于后续功能。
- 内部ID与时间为数值绑定，cursor所有字段必须校验；filter_key基于规范化filter的SHA-256，用于一致性校验，不承担鉴权。
- upstream_started是boolean或null；进行中/崩溃恢复尚不能证明是否发上游时为null。只有明确false才显示“未发上游”，未知不能当作未消费。
- source=connection_test且主动首事件探测结束的cancelled，在UI标“探测提前结束”；这个统计状态不代表连接诊断失败。

## 6. stats_request_detail

请求：`{"command":"stats_request_detail","request_tag":"detail-1","request_id":"req-a"}`。

返回 request（列表同结构）以及 parser_version、created_at_ms、updated_at_ms、分项cost、本次独立保存的单价快照（不关联当前价格）。请求被清理返回stats_not_found。原始正文、Key和原始HTTP头从未存储，详情不能提供下载原文。

## 7. stats_breakdown

请求：`command,request_tag,filter,group_by,sort,limit,cursor,page?`。`page` 与明细列表的页码直达约定相同；汇总在聚合结果上定位。分组计数达到上限时总数是下界，UI 显示 `≥` 并仍允许继续翻页或直接跳转。

- group_by=provider/model；sort=total_tokens/request_count；limit默认20、最大100。
- 每组包含 key、label、provider_count、最近名称快照、与dashboard同口径summary。按排序指标DESC、key ASC稳定排序；无完整用量组排最后。
- group_by=model 的key=upstream_model，provider的key=provider_id。UI不得按显示名合并供应商。
- 返回 items、total_groups、next_cursor。游标包括sort_metric/key/as_of_id/filter_key/run_id/maintenance_revision；metric变化可能造成移动，刷新重置，不能承诺分页MVCC。
- 单次只读事务、查询2s预算、禁止返回全部100万条交给GUI聚合。

## 8. stats_filters

请求：`command,request_tag,kind,query,provider_id,limit,after_key`。

- kind=provider/model；query为0..80字符的字面子串检索（LIKE中的 `%`/`_` 转义），不能注入SQL。
- limit默认50最大100；按稳定key ASC分页，返回items=[{key,label,historical}]、next_key。
- provider选项合并当前已保存配置与历史记录；model选项同理，可由provider_id收窄。去重和label取值规则见需求。
- 不暴露地址或凭据，不用全量配置JSON做统计筛选返回。

## 9. P1设置和维护命令

### stats_settings_get / stats_settings_update

get返回 enabled、retention_days、default_currency、revision；update请求包含 expected_revision 和完整的新 enabled/retention_days/default_currency。写线程事务中校验版本，更新后 revision+1，成功再更新核心原子快照。只有成功后GUI才改显示。暂停只影响新请求，已接纳Guard继续结算。

### stats_provider_settings_get / stats_provider_settings_update

请求包含provider_id；get不存在时返回usage_mode=observe/revision=0。update包含expected_revision、usage_mode=observe/request；只允许当前配置存在的供应商。事务提交成功后刷新新的请求设置快照。配置删除不级联删除历史。

### stats_prices_list / stats_price_save / stats_price_delete

价格业务以[PRICING-DECISION.md](PRICING-DECISION.md)为准。schema_v2中的stats_model_prices是可修改当前配置；(provider_id,upstream_model)组合唯一。所有rate_nanos传整数十进制字符串，未填项保存0。

list支持provider_id/upstream_model、limit<=100和after_price_id分页，返回price_id、四项单价、currency、input_mode、label、revision、created/updated时间。不返回生效区间，不按历史引用锁定编辑。

save参数：price_id（新建null）、provider_id、upstream_model、currency、input_mode、四项rate_nanos、label、expected_revision（新建0）。更新精确匹配price_id+revision，成功revision+1；重复组合键报冲突。事务提交后发布新价格缓存，已经接纳的请求保留自己的快照。

delete参数：price_id、expected_revision。可以删除已被历史请求使用过的配置；不扫描/修改历史费用，删除后新请求采用0价格。

费用DTO新增本次四项单价快照、price_source、snapshot时间、可选source_price_id/revision；不以来源ID查当前价。无价格的终态cost.status=default_zero，单价/分项/总金额全部字符串"0"，currency为当次默认币种；有手动非零价但用量不全仍为usage_incomplete且金额null。

summary中的unpriced_request_count改为default_zero_request_count；default_zero金额参与按币种统计。设置get/update增加default_currency，CNY/USD，修改不回写历史。

### stats_reprice_start（停用）

不再提供按当前价格改写历史的能力，调用返回`stats_operation_removed`。GUI移除重算入口；历史维护日志仍可只读查看。不要把新价格补到原本默认0的历史记录。


### stats_clear_start

先stats_clear_preview参数before_ms，返回待删行数与确认token（有效60秒、绑定边界/as_of_id/run_id）。用户确认后start携带token；只删除 id<=as_of_id、ended_at_ms非空且started_at_ms<before_ms 的行。不能通过直接调用start省略确认，token只在当前进程内有效。不删除设置、价格和进行中。

### stats_maintenance_status

参数task_id，返回kind/status/affected_rows/started/completed/error_code。一次只运行一个清理/保留任务，另一个返回stats_busy；分批释放写线程，不饿死聊天事件。

自动retention无UI确认，按已保存的保留策略执行。手动clear有确认。维护任务的进度以已提交批次数量为准，重启时running任务标failed，已完成批次有效，可安全重试；第一版不承诺从精确断点自动恢复。

## 10. GUI Backend接入规则

不能把统计命令原样交给现有 `send()`：它会设置全局busy，且Backend对非poll/metadata任务会重载config并再取snapshot。

增加结构化Job类别或专用StatsBackend：有限队列、3s查询超时、统计DTO、独立Reply分支，统计查询不加载config、不调用snapshot、不触发capture/save、不覆盖代理状态。保持现有控制连接 `.proxy(None)`、禁止重定向和message大小限制。

统计只读命令加入 `Bridge::command` 的静默命令判定，不能每10秒污染界面事件日志。统计写操作返回维护/设置结果，不改普通转发status字符串。

独立 WorkBuddy 收费项的 `usage_status` 为 `not_applicable`，Token 列保持 NULL，不计入 `usage_missing_count`、缺失告警或 `usage_eligible_count`；积分仍参与各层级总计。既有记录在 schema v11 升级时修正。
