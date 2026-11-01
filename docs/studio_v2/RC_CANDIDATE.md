# Studio V2 0.6.0 候选审查

2026-11-01用户已明确要求快速推进，直接更新 Release、README 最新 V2 截图及 main。当前按 0.6.0 执行[快速发布清单](FAST_RELEASE_CHECKLIST.md)，冻结最终源码后利用 main 自动 CI 生成并核对最终包，再更新公开 Release。下述 QA91 原包只保留原来源，不冒充包含 UI855 修复的新包。

## 候选内容

- Core 与设置界面的 Cargo 版本统一为 0.6.0，依赖版本和业务／渲染源码保持本轮变更前的状态。
- 包名、Mac 主程序／helper 的 plist 和 Windows 安装器版本来自同一清单，不接受另一个旧版本标签。升级工具独立使用冻结清单中的候选版本，不改写历史包身份。
- 发行说明草案见 [CHANGELOG](../../CHANGELOG.md)，随包附带[完整备份与冷回滚步骤](UPGRADE_ROLLBACK.md)。历史测试候选 9570451／0.5.7 保留原证据来源，未冒充本次候选。
- 沿用项目现有 Mac ad-hoc／未公证、Windows unsigned 分发方式；真实下载后的 Gatekeeper／SmartScreen 行为仍需验收。

## 集中验证

先完成版本防漂移测试、锁定元数据与本机 Core／UI Release 版本核对，再冻结 Git 源码。对同一冻结源只派发一轮 Core 检查及三平台完整 Desktop 矩阵，保留首次结果、安装器、原生生命周期、Windows GDI 和包级报告。失败时按实际原因修复，不对同一失败源码反复重跑到绿。

完整矩阵通过后绑定实际候选包的来源、大小和 SHA256，再补旧版包升级／冷回滚复核。候选包和校验清单作为审查交付物保留，不将 CI artifact 的临时保留期限当作正式长期分发。

## 尚未签收的门禁

[release_plan.json](release_plan.json) 是逐项状态来源。R01 的真实悬停／按下／拖列、逐引擎输入法和辅助技术，R02 的真实设备、系统交互与缩放，R03 的有效前台、启动、峰值及最终性能结论均继续保留。编译、版本 CLI、后台样本、Rosetta 或自动化生命周期不能替代这些证据。

40 帧签收保持。用户已授权公开发布；按快速清单保留最终 CI、真实包冒烟和数据保护检查，延后专项明确列为未验证，不将原完整门禁改为测试通过。

## QA92 后的来源区别

当前 UI 已因全屏顶栏背景错位修复变为 `85540e1`，本机输入程序 SHA256 为 `f756a70901da78e19185b9b9c1e4a29b1cfaaa0ee8fce35b0f8f2cd7772c491c`，Core 仍沿用 dd9 的 b920。原生红绿与用户确认见[全屏修复](benchmarks/manual-mac-rc060/fullscreen-red-green-review.json)。这是新的 UI 阶段候选；最终完整矩阵因此重新待验。

下述 dd9 四个已保留包及升级／回滚结果仍有效，但仅证明那些原包，不能冒称包含本次 UI 修复的新最终安装包。阶段检查不会重复未改 Core 或原包六例升级；正式出厂时再冻结实际最终包并绑定相应门禁。

## QA91 可审查候选包（原来源保留）

冻结运行／打包源为 `dd9debade83e91b0d11c6b28f18f97edb227abbd`。唯一 [Core 检查](https://github.com/gloryhui/workbuddy-proxy/actions/runs/41278921019) 与[三平台完整 Desktop](https://github.com/gloryhui/workbuddy-proxy/actions/runs/41278929532) 均首轮成功，原始报告和产物来源见[矩阵核验](benchmarks/rc-060/ci/desktop/validation.json)。这不替代前述人工、硬件与性能门禁。

实际下载校验后的 Windows ZIP／setup、Mac ARM／Intel DMG 及各自校验文件保留于本地仓库的 `dist/studio-v2-rc/0.6.0`，四包共 140,896,314 字节。[候选清单](benchmarks/rc-060/candidate-packages.json)记录每包大小与 SHA256；该目录是审查交付物，不提交大二进制，也不当临时文件删除。CI artifact 和本地保留包均不冒称公开 Release 下载。

Windows 原生两尺寸[原图审查](benchmarks/rc-060/windows-visual-review.json)确认版本与标题栏分离，均为隔离合成数据；尚未替换公开 README 图片。首次测试源 `21dea55` 的[升级 run](https://github.com/gloryhui/workbuddy-proxy/actions/runs/41281534447)在 Intel 冷快照因非空 WAL 断言失败，原证据保留。修正测试快照读取后，新测试源 `c435b5c` 的[升级／回滚 run](https://github.com/gloryhui/workbuddy-proxy/actions/runs/41282248853)三平台首轮通过；[六例核验](benchmarks/rc-060/ci/upgrades-final/validation.json)确认原包和完整冷备份恢复、候选新记录独立保留。[WAL 诊断](benchmarks/rc-060/cold-snapshot-diagnosis.json)说明守卫红绿与范围；产品程序没有为此重建。
