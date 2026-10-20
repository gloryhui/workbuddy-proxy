# D15 · 统计页面默认5秒刷新

2026-10-20，Codex 直接实现与复核，基线 `68ca6ff`。用户最终确认：后台 WorkBuddy JSONL 采集保留当前固定5秒。页面查询已采集统计数据的刷新设置是独立链路。

## 改动

只改 `gpui/src/statistics.rs`：页面刷新默认0→5秒；中英 tooltip 解释仅刷新已采集数据、不改变后台采集频率；aria 改为统计页面自动刷新间隔。选项仍为关闭/5/10/30/60秒，原前台/弹窗/请求在途保护保持。

`tick → refresh → stats_dashboard/rows/groups/health` 查询既有统计数据；后台读取由 `reader::SYNC_INTERVAL`、observer/scheduler 独立驱动，固定5秒。本轮根 src 与 core 二进制无变更。导入状态等既有独立轮询保持，不把页面刷新设置描述为所有 SQLite 查询的总开关。

## 实测

- 格式检查通过，现有 GPUI 测试 **139 passed**，release构建通过；见 [logs](logs/results.json)、[测试](logs/tests.txt)、[构建](logs/build.txt)。未新增常量镜像测试。
- macOS/CUA 隔离实例默认显示5秒；菜单仍有全部5项。
- 合成SQLite 20→21条，约一个刷新周期后页面无需手动操作即为21。
- 选择关闭；再插入第22条，等待超过5秒，页面仍21；恢复5秒，再等待后页面为22。
- [AX/截图](ax/06-resumed-22.txt)与数据库插入记录 [insertions.jsonl](logs/insertions.jsonl)交叉验证。后台采集固定周期是源码保持验证，未以该合成测试冒充真实 WorkBuddy 文件采集计时。
- tooltip 文案在源码核对；截图07未展示悬停提示，不将其当作提示浮层视觉证据。英语、Windows、Retina/IME本轮未测。
- 未保存配置/同步/改端口；假配置开始/结束哈希相同。真实用户配置、凭据、WorkBuddy历史未触碰。

## 清理

构建、GUI子进程终止并wait后，按精确CAP、owner与lsof引用检查删除临时配置、SQLite、app副本、mock与临时文件。详情见两份cleanup JSON；会话tmp为空、保留0字节；持久编译缓存与证据保留。删除逻辑字节与系统余量见results.json（不等同实际磁盘回收）。旧机`/Volumes/1TB`未使用。
