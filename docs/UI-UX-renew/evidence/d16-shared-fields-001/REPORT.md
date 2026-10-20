# D16 · 共享输入控件 36px

2026-10-20，Codex 直接实现与复核；基线 `40dadaa`。用户接受共享层统一 36px；映射输入也统一，按钮仍为原有尺寸。

## 实现

Kit 0.7.3 的单行 Input 忽略其固有 `Input::h`（该 API 用于多行）；共享 helper 改用 `Styled::h`，Select 用样式高度。应用自有 Input/Select 构造均收敛到共享 helper，覆盖供应商、模型选择/编辑、四种协议、统计筛选/日期/币种、定价搜索/新建/编辑。状态 Entity、事件、禁用条件、前缀与 aria 保留；未修改 Kit、全局 rem、字体或业务逻辑。

筛选槽先前仅 32px，现为 36px。GUI 发现首版横向居中缩窄日期/刷新按钮，已改纵向 flex 保持横向铺满。最终构建与复验均在该修复后。

## 验证

- `cargo fmt --manifest-path gpui/Cargo.toml --check` 与 `git diff --check` 通过。
- 最终现有 GPUI 测试 **139 passed**，见 [tests.txt](logs/tests.txt)；release [build.txt](logs/build.txt) 通过。
- 最终二进制 SHA256 `269f8b03f19fce436dde4fe16366ded3cb271f43537e3c614706deeae5d8aeb4`。`*-initial.txt` 属首轮构建，已被最终证据取代。
- macOS CUA 原生截图/AX：供应商；OpenAI Chat/Responses、Anthropic、Gemini；模型 picker 搜索；模型编辑；更多筛选；日期；定价搜索、新建与编辑。见 `shots/01`—`15` 和对应 `ax/`。输入框外缘高度 36px（例如供应商名称 y239—275、URL y383—419）；日期/刷新按钮保持32px且原宽度恢复。
- 供应商名称输入后恢复；模型搜索 `mock-model-a` 将两项过滤为一项；所有草稿弹窗取消。假配置开始/结束 SHA 一致（[runtime.json](logs/runtime.json)、[config-sha-final.txt](logs/config-sha-final.txt)）。未保存、同步、改用户端口或真实配置。
- **尺寸限制：**截图原生尺寸1440×901（包含窗口边界）；本轮 CUA 边框拖动未能改变窗口，zoom得到1920×991。`16`—`18` 是未成功缩小的尝试，文件名中的 small 不代表1100×700证据。因此最小窗口在36px修改后仍需补验；Windows/Retina/英文/IME亦未本轮验证，不声称完整验收。

## 隔离与清理

只使用本机会话缓存与已有 `gpui/target`、`target`；TMPDIR/TMP/TEMP 为会话专用目录。`session.py` 管理本机 loopback mock、core 和临时 app bundle，停止、wait 后按 owner/引用核对精确清理。曾因 SQLite 未释放引用而保留首次 CAP，进程退出后复核再清理；记录见 cleanup JSON。

最终 CAP 已删除：两个构建目录各21字节；三个 GUI CAP 分别259782、15562462、15570691字节，共31392977字节（约29.94MiB，文件逻辑大小；不代表磁盘实际回收）。会话 tmp 当前为空。持久编译缓存、证据保留。系统盘剩余约194.8GiB。系统临时目录归属不明确的历史内容未删除。
