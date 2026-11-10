//! 设置页的供应商/配置命令传输层。
//!
//! 上游调用方是 `ui::Proxy` 的按钮、快捷键和轮询逻辑；下游是本机 Core 的
//! `/_control/action` 管理接口，以及仅在设置进程内执行的供应商创建流程和
//! 模型目录缓存。`Backend::new` 启动名为 `settings-control` 的标准线程，
//! 每个 `Job` 在后台按顺序执行；UI 线程只负责 `try_send` 入队，并在
//! `Proxy::tick` 中非阻塞排空 `Reply`。配置、草稿、Entity 和渲染状态仍归
//! UI 线程所有，后台线程不会直接调用 `cx.notify()`。

use crate::config::Config;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::mpsc::{self, Receiver, SyncSender},
    time::Duration,
};

pub struct Job {
    /// UI 消费回复时使用的动作名，例如 `save`、`test`、`poll`。部分
    /// `*_restart` 名称会在 worker 中映射回 UI 能识别的标签。
    pub tag: String,
    /// 按顺序执行的管理命令；通常为“保存草稿 + 同步/测试”等步骤。
    pub steps: Vec<Value>,
}
pub struct Reply {
    /// 已完成的工作类型，决定 `Proxy::tick` 如何合并结果和刷新状态。
    pub tag: String,
    /// 成功值是 Core `message` 字符串；某些动作的字符串内部仍是 JSON。
    /// 失败值已包含可直接展示/本地化的 Core 错误边界。
    pub result: Result<String, String>,
    /// 仅在后续读取成功时存在。`None` 表示不可信任的配置快照，调用方
    /// 不应拿旧草稿猜测磁盘内容。
    pub config: Option<Config>,
    /// 除 `poll` 外，普通动作完成后额外读取的实时 Core snapshot；`poll`
    /// 则直接把本次只读结果解析到这里。`metadata`、`workbuddy_check` 与
    /// `update_status` 明确不附带 snapshot，因此 `None` 不能解释成 Core
    /// 不可达，也不能被其他动作照搬为通用失败信号。
    pub snapshot: Option<Value>,
    /// 最后的配置命令确实已成功落盘（安装/恢复模型或设置端口）。即使
    /// 随后的进程重启失败，该值仍为真，以便 UI 刷新已提交的配置。
    pub synced: bool,
    /// 新供应商创建流程的最终状态；普通保存/测试为 `None`。
    pub creation: Option<crate::provider_create::Outcome>,
    /// 本次请求中成功保存的真实供应商 ID。用于在“保存成功、后续动作
    /// 失败”时仍允许合并已落盘字段。
    pub saved_provider: Option<String>,
    /// 显式连接测试的窗口内测试收据；普通请求为 `None`。
    pub test_feedback: Option<crate::provider_connection::Feedback>,
}
pub struct Backend {
    /// 有界同步 channel（容量 8）。UI 使用 `try_send`，排队满时立即把
    /// 控制权还给界面并显示“后台忙”，不会阻塞渲染线程。
    pub jobs: SyncSender<Job>,
    /// worker 到 UI 的有界回执队列（容量 8）。UI 在每 250ms tick 中调用
    /// `try_recv` 排空，完成动作后再更新 `Proxy`。
    pub replies: Receiver<Reply>,
}

/// 在 `settings-control` 线程内执行一个 Job。
///
/// `call` 执行每个管理步骤；任一步失败会立即停止，不会执行剩余步骤。
/// `restart` 只在一组特定动作的最后配置步骤成功后才调用。返回值的第一项
/// 是最终用户可见结果，第二项 `committed` 表示配置是否已经成功写盘。
///
/// 典型边界：
/// - 保存后同步失败：保存步骤可能已改变 Core 配置，但 `committed` 为假；
///   worker 后续仍会读取配置并附带 `saved_provider`，由 UI 决定可合并范围。
/// - 同步/恢复/端口设置成功、重启失败：返回错误，但 `committed` 为真，
///   因为错误文案也明确告知用户磁盘配置已经更新。
fn execute(
    job: &Job,
    mut call: impl FnMut(&Value) -> Result<String, String>,
    restart: impl FnOnce() -> Result<(), String>,
) -> (Result<String, String>, bool) {
    // 所有步骤共享同一顺序。闭包捕获在 worker 线程内执行，因而网络读取和
    // 文件操作不会占用 GPUI 的 UI/HTTP 回调线程。
    let mut result = Ok(String::new());
    for step in &job.steps {
        result = call(step);
        if result.is_err() {
            return (result, false);
        }
    }
    let last_command = job.steps.last().and_then(|step| step["command"].as_str());
    let port_change = job.tag == "port_restart";
    // 只有真正改变 Core 配置的最后一步才能算“已提交”。普通只读查询即使
    // 成功也不会把草稿当作已保存。
    let committed = matches!(last_command, Some("install" | "restore"))
        || (port_change && last_command == Some("set_port"));
    let restart_after = match job.tag.as_str() {
        "install_restart" | "startup_install_restart" => Some("install"),
        "restore_restart" => Some("restore"),
        "port_restart" => Some("set_port"),
        _ => None,
    };
    if let Some(required) = restart_after {
        if !committed || last_command != Some(required) {
            return (
                Err(
                    "Restart requires a successful configuration update / 重启前必须成功更新配置"
                        .into(),
                ),
                false,
            );
        }
        let port_result = result.as_ref().ok().cloned().unwrap_or_default();
        result = restart().map(|()| {
            if port_change {
                let mut value: Value = serde_json::from_str(&port_result).unwrap_or_else(|_| json!({}));
                value["workbuddy_restarted"] = json!(true);
                value.to_string()
            } else if job.tag == "restore_restart" {
                "Proxy models removed; WorkBuddy reopened / 已取消同步并重新打开 WorkBuddy".into()
            } else {
                "Models synced; WorkBuddy reopened / 已同步模型并重新打开 WorkBuddy".into()
            }
        }).map_err(|e| {
            if port_change {
                format!("Local port and saved addresses updated; {} / 本地端口和已保存的地址已更新；{}", crate::i18n::message(&e, false), crate::i18n::message(&e, true))
            } else if job.tag == "restore_restart" {
                format!("Proxy models removed from disk; {} / 已取消同步并写入文件；{}", crate::i18n::message(&e, false), crate::i18n::message(&e, true))
            } else {
                format!("Models synced to disk; {} / 模型已同步到文件；{}", crate::i18n::message(&e, false), crate::i18n::message(&e, true))
            }
        });
    }
    (result, committed)
}
pub fn call(dir: &std::path::Path, payload: &Value) -> Result<String, String> {
    call_detailed(dir, payload).map_err(|failure| failure.message)
}
/// 发送一条本机管理命令，并保留“确定失败/结果不确定”的创建语义。
///
/// `instance.json` 提供 loopback 端口与本次 Core 实例的管理 key。请求固定
/// 发往 `127.0.0.1`，禁用环境代理和重定向，避免凭证或配置命令离开本机。
/// 普通命令最多等待 100 秒；只读快照与 WorkBuddy 地址检查为 3 秒。回复体
/// 上限为 1 MiB，超过或其他 JSON 解析失败会形成协议错误。
///
/// 失败分类不是业务重试策略：连接明确被拒绝等错误标为 `certain`；超时、
/// 服务端错误或无法判断请求是否已执行的情况标为 `uncertain`。调用方可在
/// 创建供应商等有副作用操作中据此避免盲目重试。
fn call_detailed(
    dir: &std::path::Path,
    payload: &Value,
) -> Result<String, crate::provider_create::Failure> {
    use crate::provider_create::Failure;
    let info: Value = serde_json::from_slice(
        &fs::read(dir.join("instance.json"))
            .map_err(|_| Failure::certain("Core is not responding"))?,
    )
    .map_err(|_| Failure::certain("Invalid core response"))?;
    let port = info["port"]
        .as_u64()
        .filter(|p| *p > 0 && *p <= 65535)
        .ok_or_else(|| Failure::certain("Invalid core response"))?;
    let key = info["management_key"]
        .as_str()
        .ok_or_else(|| Failure::certain("Invalid core response"))?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .proxy(None)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(
            if matches!(
                payload["command"].as_str(),
                Some("snapshot" | "workbuddy_check")
            ) {
                3
            } else {
                100
            },
        )))
        .build()
        .into();
    let mut response = agent
        .post(format!("http://127.0.0.1:{port}/_control/action"))
        .header("Authorization", format!("Bearer {key}"))
        .send_json(payload)
        .map_err(|error| {
            let certain = matches!(&error, ureq::Error::ConnectionFailed)
                || matches!(&error, ureq::Error::Io(e) if matches!(e.kind(), std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::AddrNotAvailable));
            if certain { Failure::certain("Core is not responding") } else { Failure::uncertain("Core is not responding") }
        })?;
    let success = response.status().is_success();
    let rejected = response.status().is_client_error();
    let data: Value = response
        .body_mut()
        .with_config()
        .limit(1024 * 1024)
        .read_json()
        .map_err(|_| {
            if rejected {
                Failure::certain("Invalid core response")
            } else {
                Failure::uncertain("Invalid core response")
            }
        })?;
    let message = data["message"]
        .as_str()
        .unwrap_or("Invalid core response")
        .to_owned();
    if success {
        Ok(message)
    } else if rejected {
        Err(Failure::certain(message))
    } else {
        Err(Failure::uncertain(message))
    }
}
impl Backend {
    pub fn new(dir: PathBuf) -> Self {
        // `sync_channel` 提供有界队列。`(jobs, requests)` 中发送端 `jobs`
        // 存入 `Backend`，接收端 `requests` 被移动到 worker 闭包；
        // `(results, replies)` 中 `results` 是 worker 发送端，`replies` 是
        // UI 持有的接收端。两个线程通过消息传递共享工作，不共享 `Proxy`。
        let (jobs, requests) = mpsc::sync_channel::<Job>(8);
        let (results, replies) = mpsc::sync_channel(8);
        std::thread::Builder::new()
            .name("settings-control".into())
            .stack_size(512 * 1024)
            .spawn(move || {
                // 该缓存只在 settings worker 生命周期内存在，用于把一次模型
                // 发现结果复用到后续 metadata 请求；它不是 Core 配置，也
                // 不会跨进程共享。
                let mut draft_catalog: Option<crate::draft_discovery::Cache> = None;
                // `recv()` 阻塞等待 UI 入队。发送端随 `Backend` 一起释放时，
                // 循环退出，线程自然结束；UI 重绘不依赖该线程执行。
                while let Ok(job) = requests.recv() {
                    let mut creation = None;
                    let mut saved_provider = None;
                    let mut test_feedback = None;
                    let (mut result, synced) = execute(
                        &job,
                        |step| {
                            // 少数命令只在设置进程内完成，不直接转发给 Core。
                            // 新供应商则由 `commit` 先校验、再创建、再绑定真实 ID
                            // 并保存；全部步骤仍在该 worker 上串行发生。
                            if step["command"] == "ui_discover_models" {
                                let (ids, cache) = crate::draft_discovery::fetch(step)?;
                                draft_catalog = Some(cache);
                                Ok(ids)
                            } else if step["command"] == crate::provider_create::COMMAND {
                                let pending = serde_json::from_value(step["pending"].clone()).map_err(|_| "Invalid local draft request")?;
                                let draft = serde_json::from_value(step["draft"].clone()).map_err(|_| "Invalid local draft request")?;
                                let (result, outcome) = crate::provider_create::commit(
                                    &pending, &draft, || Config::load(&dir), |payload| call_detailed(&dir, payload),
                                );
                                if let Some(cache) = &mut draft_catalog
                                    && let Some(id) = &outcome.provider_id {
                                    cache.rebind(&outcome.local_id,id);
                                }
                                if outcome.saved {
                                    saved_provider = outcome.provider_id.clone();
                                    // Save already starts the core's metadata refresh.
                                    // Subsequent UI reads must use that same resolver,
                                    // including its fallback when refreshing fails.
                                    draft_catalog = None;
                                }
                                creation = Some(outcome);
                                result
                            } else {
                                let mut payload = step.clone();
                                if let Some(outcome) = &creation
                                    && payload["provider_id"].as_str() == Some(&outcome.local_id)
                                    && let Some(id) = &outcome.provider_id {
                                    // UI 草稿先用本地 ID；创建成功后，同一次 Job
                                    // 的后续测试/保存必须替换成 Core 生成的真实 ID。
                                    payload["provider_id"] = json!(id);
                                }
                                if payload["command"] == "model_metadata"
                                    && let Some(cache) = draft_catalog.as_ref().filter(|cache|cache.matches(&payload)) {
                                    // Resolve public/builtin metadata without a stale live
                                    // credential cache, then apply the unsaved discovery.
                                    let mut base = payload.clone();
                                    base["key"] = json!(format!("ui-resolver-{}", &crate::config::token()[..12]));
                                    let mut value: Value = serde_json::from_str(&call(&dir,&base)?).map_err(|_|"Invalid model metadata")?;
                                    cache.enrich(&payload,&mut value["metadata"])?;
                                    Ok(value.to_string())
                                } else {
                                    let result=call(&dir, &payload);
                                    if job.tag == "test" && payload["command"] == "test" {
                                        // 在错误被“已保存但后续失败”包装前捕获测试
                                        // 报告，避免把保存状态误当作连接测试结论。
                                        test_feedback = Some(crate::provider_connection::Feedback::from_result(
                                            payload["provider_id"].as_str().unwrap_or(""), &result,
                                        ));
                                    }
                                    if result.is_ok() && payload["command"] == "save" {
                                        saved_provider=payload["provider_id"].as_str().map(str::to_owned);
                                    }
                                    result
                                }
                            }
                        },
                        crate::workbuddy_restart::restart,
                    );
                    if saved_provider.is_some()
                        && let Err(error) = &mut result {
                        // 保存已成功而同步/重启后失败时必须保留这一事实，
                        // 否则 UI 会把已落盘的配置误当成完全未保存。
                        *error = format!("Provider saved; subsequent action failed: {} / 供应商已保存，后续操作失败：{}",crate::i18n::message(error,false),crate::i18n::message(error,true));
                    }
                    // `poll` 的 snapshot 来自同一次只读查询；其他动作通常再读
                    // 一次 snapshot。模型目录、地址检查和更新检查刻意不附带，
                    // 因为它们不改变展示供应商配置快照的所有权。
                    let snapshot = if matches!(job.tag.as_str(), "metadata" | "workbuddy_check" | "update_status") {
                        None
                    } else if job.tag == "poll" {
                        result
                            .as_ref()
                            .ok()
                            .and_then(|s| serde_json::from_str(s).ok())
                    } else {
                        call(&dir, &json!({"command":"snapshot"}))
                            .ok()
                            .and_then(|s| serde_json::from_str(&s).ok())
                    };
                    let config =
                        if !matches!(job.tag.as_str(), "metadata" | "workbuddy_check" | "update_status") {
                            if job.tag == "poll" || (job.tag == "delete" && result.is_err()) {
                                // Polls only read an existing, bounded, migrated
                                // configuration. Never initialize or rewrite it.
                                // 失败删除则用只读读取得到 Core 的实际配置，供 UI
                                // 把本次操作误以为成功前的界面收敛到可信状态。
                                crate::config_refresh::existing_config(&dir)
                            } else {
                                Config::load(&dir).ok()
                            }
                        } else {
                            None
                        };
                    if results
                        .send(Reply {
                            // 重启类 tag 通过 UI 对外收敛为普通的 install/port/
                            // restore/startup_install；`synced` 单独保留提交事实。
                            tag: if job.tag == "install_restart" {
                                "install".into()
                            } else if job.tag == "port_restart" {
                                "port".into()
                            } else if job.tag == "restore_restart" {
                                "restore".into()
                            } else if job.tag == "startup_install_restart" {
                                "startup_install".into()
                            } else {
                                job.tag
                            },
                            result,
                            config,
                            snapshot,
                            synced,
                            creation,
                            saved_provider,
                            test_feedback,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .expect("Cannot start settings worker");
        Self { jobs, replies }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    #[test]
    fn unsync_restart_requires_restore_and_reports_partial_success() {
        let mut job = Job {
            tag: "restore_restart".into(),
            steps: vec![json!({"command":"restore"})],
        };
        let restored = std::cell::Cell::new(false);
        let (result, committed) = execute(
            &job,
            |step| {
                assert_eq!(step["command"], "restore");
                restored.set(true);
                Ok("removed".into())
            },
            || {
                assert!(restored.get());
                Ok(())
            },
        );
        assert!(committed);
        assert!(result.unwrap().contains("已取消同步并重新打开 WorkBuddy"));
        let (result, committed) = execute(
            &job,
            |_| Err("restore failed".into()),
            || panic!("restore failed; must not restart"),
        );
        assert_eq!(result, Err("restore failed".into()));
        assert!(!committed);
        let (result, committed) = execute(
            &job,
            |_| Ok("removed".into()),
            || Err("quit refused / 拒绝退出".into()),
        );
        assert!(committed);
        assert!(
            result
                .unwrap_err()
                .contains("已取消同步并写入文件；拒绝退出")
        );
        job.tag = "restore".into();
        assert!(
            execute(
                &job,
                |_| Ok("removed".into()),
                || panic!("remove-only never restarts")
            )
            .0
            .is_ok()
        );
        job.tag = "restore_restart".into();
        job.steps = vec![json!({"command":"install"})];
        assert!(
            execute(
                &job,
                |_| Ok("wrong action".into()),
                || panic!("install is not restore")
            )
            .0
            .is_err()
        );
    }

    #[test]
    fn port_restart_occurs_only_after_success_and_keeps_committed_state_on_restart_failure() {
        let job = Job {
            tag: "port_restart".into(),
            steps: vec![json!({"command":"set_port","port":19000})],
        };
        let (result, committed) = execute(
            &job,
            |_| Err("occupied".into()),
            || panic!("failed port must never restart"),
        );
        assert_eq!(result, Err("occupied".into()));
        assert!(!committed);
        let applied = std::cell::Cell::new(false);
        let (result, committed) = execute(
            &job,
            |_| {
                applied.set(true);
                Ok(json!({"port":19000,"changed":true,"updated":2}).to_string())
            },
            || {
                assert!(applied.get());
                Ok(())
            },
        );
        let value: Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert!(committed);
        assert_eq!(value["port"], 19000);
        assert_eq!(value["workbuddy_restarted"], true);
        let (result, committed) = execute(
            &job,
            |_| Ok("applied".into()),
            || Err("quit refused / 拒绝退出".into()),
        );
        assert!(committed);
        assert!(
            result
                .unwrap_err()
                .contains("本地端口和已保存的地址已更新；拒绝退出")
        );
        let empty = Job {
            tag: "port_restart".into(),
            steps: vec![],
        };
        assert!(
            execute(&empty, |_| unreachable!(), || panic!("nothing applied"))
                .0
                .is_err()
        );
    }

    #[test]
    fn startup_sync_never_saves_drafts_or_restarts_before_sync() {
        let job = Job {
            tag: "startup_install_restart".into(),
            steps: vec![json!({"command":"install"})],
        };
        let synced = std::cell::Cell::new(false);
        let (result, committed) = execute(
            &job,
            |step| {
                assert_eq!(step["command"], "install");
                synced.set(true);
                Ok("synced".into())
            },
            || {
                assert!(synced.get());
                Ok(())
            },
        );
        assert!(result.is_ok() && committed);
        assert!(
            execute(
                &job,
                |_| Err("sync failed".into()),
                || panic!("sync failed")
            )
            .0
            .is_err()
        );
    }

    #[test]
    fn restart_requires_every_save_and_sync_to_succeed() {
        let job = Job {
            tag: "install_restart".into(),
            steps: vec![json!({"command":"save"}), json!({"command":"install"})],
        };
        for fail_at in [0, 1] {
            let mut calls = 0;
            let (result, synced) = execute(
                &job,
                |_| {
                    let current = calls;
                    calls += 1;
                    if current == fail_at {
                        Err("fixture failure".into())
                    } else {
                        Ok("saved".into())
                    }
                },
                || panic!("must not restart after failure"),
            );
            assert_eq!(result, Err("fixture failure".into()));
            assert!(!synced);
            assert_eq!(calls, fail_at + 1);
        }
        let calls = std::cell::Cell::new(0);
        let (result, synced) = execute(
            &job,
            |_| {
                calls.set(calls.get() + 1);
                Ok("ok".into())
            },
            || {
                assert_eq!(calls.get(), 2);
                Ok(())
            },
        );
        assert!(result.is_ok() && synced);
        let (result, synced) = execute(
            &job,
            |_| Ok("ok".into()),
            || Err("quit refused / 拒绝退出".into()),
        );
        assert!(synced, "saved config must refresh even if restart fails");
        assert!(result.unwrap_err().contains("模型已同步到文件；拒绝退出"));
        let sync_only = Job {
            tag: "install".into(),
            steps: job.steps,
        };
        assert!(
            execute(
                &sync_only,
                |_| Ok("synced".into()),
                || panic!("sync-only must never restart")
            )
            .0
            .is_ok()
        );
        let empty = Job {
            tag: "install_restart".into(),
            steps: vec![],
        };
        assert!(
            execute(&empty, |_| unreachable!(), || panic!("no sync occurred"))
                .0
                .is_err()
        );
    }
    #[test]
    fn authenticated_local_control_handles_success_and_failure() {
        let dir = tempfile::tempdir().unwrap();
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        fs::write(
            dir.path().join("instance.json"),
            json!({"port":socket.local_addr().unwrap().port(),"management_key":"fixture-token"})
                .to_string(),
        )
        .unwrap();
        let worker = std::thread::spawn(move || {
            for status in [200, 400] {
                let (mut stream, _) = socket.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0u8; 1024];
                loop {
                    let size = stream.read(&mut buffer).unwrap();
                    assert!(size > 0);
                    bytes.extend_from_slice(&buffer[..size]);
                    if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                        let length = headers
                            .lines()
                            .find_map(|s| s.strip_prefix("content-length:"))
                            .unwrap()
                            .trim()
                            .parse::<usize>()
                            .unwrap();
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                let request = String::from_utf8_lossy(&bytes).to_lowercase();
                assert!(request.contains("authorization: bearer fixture-token"));
                let body = r#"{"message":"fixture"}"#;
                write!(
                    stream,
                    "HTTP/1.1 {status} OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        assert_eq!(
            call(dir.path(), &json!({"command":"clear_logs"})),
            Ok("fixture".into())
        );
        assert_eq!(
            call(dir.path(), &json!({"command":"clear_logs"})),
            Err("fixture".into())
        );
        worker.join().unwrap();
    }
}
