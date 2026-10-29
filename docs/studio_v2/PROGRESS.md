# Studio V2 进度页维护

入口：[UI 改造进度时间线](progress_timeline.html)。页面沿用用户指定的 StreamDesk 时间线样式，内容来自本仓库的 QA、40 帧映射及 Git 提交记录。HTML 内嵌数据，离线打开即可查看；JSON 是同一份可读数据。

## 每项任务完成后

1. 将真实结果、截图、性能证据与未完成范围写入 `docs/studio_v2/baseline_and_qa.md`，使用既有 `## 小功能 N：标题` 格式。同步 `figma_node_map.md`，保留“未验收 / 部分 / 共用 / 已验收”的准确口径。
2. 验证通过后提交该项代码、证据和 QA。生成器用 QA 标题行的 Git 归属取得真实提交，不把未提交的候选标为已提交。
3. 在仓库根目录刷新进度，例如：

   ```sh
   python3 scripts/update_studio_progress.py \
     --task 31 --status recorded \
     --title '本次实际完成的任务标题' \
     --description '本次完成范围、验证结果及剩余限制' \
     --note '完成 UI-31；同步实际验收范围'
   ```

4. 检查 HTML 与 JSON 的数量、当前任务、证据链接，提交这两份更新，再一并推送。随后开始下一项时使用 `--task N --status developing --title ... --description ...`，不会抹掉已提交历史。仅同步 QA / 映射可以不带参数。

`recorded` 只表示分项已提交，不等于整帧或整项目最终验收。生成器只读本地 Git，不访问远端、不自动提交推送，不调用核心，不包含用户配置或密钥。不要手工改内嵌 JSON；下次生成会覆盖它。未识别的画板状态或没有已提交 QA 的 `recorded` 任务会拒绝更新。

刷新前核对 Goal 面板的实际状态，并用 `--goal-status active`、`paused`、`blocked` 或 `complete` 记录。该参数只更新报告，不控制 Goal；省略时保留上次记录，首次缺省为未知。Goal 暂停后本轮仍可按用户明确的“继续”指令施工，分项已提交也不表示自动 Goal 已恢复运行。

## 当前验证边界

阶段文案根据帧映射生成：仍有“未验收”帧时显示开发中；待初审帧归零时显示“逐帧初审已覆盖 · 专项验收与差异修复”。部分验收与复用待核对仍计入剩余工作，不因初审覆盖而标记整项目完成。

页面的静态检查与浏览器限制记录见 [静态校验](benchmarks/progress-timeline-static-check.json)、[浏览器检查](benchmarks/progress-timeline-browser-check.json)。浏览器工具拒绝本地 `file:` 地址并禁止替代路径绕过，因此尚未实测页面渲染、主题、筛选、展开与响应式效果；不以源码检查替代实际浏览器验收。
