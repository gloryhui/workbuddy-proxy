# T09 清理记录 2026-10-20 01:51:20 +0800

- 停止：GPUI pid 65419、隔离 core pid 64136（SIGTERM 后确认退出；ps 无 t09/workbuddy-proxy-gpui/core-current 进程）。
- 删除：T09 runtime 副本（`evidence/t09-cross-platform/runtime`，7.6 MB：config 副本、SQLite、home、pid/port/wid）。L05 原始保留目录未修改/未删除。
- 未新建构建 TMP：/Volumes/1TB 未挂载；本轮未构建、未运行 cargo test/clippy。
- 系统 /tmp：按 workbuddy/t08/t09/gpui/mock_catalog 名称检索无本项目遗留（其它项目文件未触碰）。
- 证据保留：3.4M（MATRIX.md、REPORT.md、shots、logs）。
- 共享缓存：`target/`、`gpui/target/`、L05 保留目录未清理。
- 系统盘：/dev/disk3s3s1   460Gi    13Gi   195Gi     7%    484k  2.0G    0%   /
