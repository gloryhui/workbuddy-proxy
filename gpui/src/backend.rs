use crate::config::Config;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::mpsc::{self, Receiver, SyncSender},
    time::Duration,
};

pub struct Job {
    pub tag: String,
    pub steps: Vec<Value>,
}
pub struct Reply {
    pub tag: String,
    pub result: Result<String, String>,
    pub config: Option<Config>,
    pub snapshot: Option<Value>,
    pub synced: bool,
    pub creation: Option<crate::provider_create::Outcome>,
    pub saved_provider: Option<String>,
}
pub struct Backend {
    pub jobs: SyncSender<Job>,
    pub replies: Receiver<Reply>,
}

fn execute(
    job: &Job,
    mut call: impl FnMut(&Value) -> Result<String, String>,
    restart: impl FnOnce() -> Result<(), String>,
) -> (Result<String, String>, bool) {
    let mut result = Ok(String::new());
    for step in &job.steps {
        result = call(step);
        if result.is_err() {
            return (result, false);
        }
    }
    let last_command = job.steps.last().and_then(|step| step["command"].as_str());
    let port_change = job.tag == "port_restart";
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
        let (jobs, requests) = mpsc::sync_channel::<Job>(8);
        let (results, replies) = mpsc::sync_channel(8);
        std::thread::Builder::new()
            .name("settings-control".into())
            .stack_size(512 * 1024)
            .spawn(move || {
                let mut draft_catalog: Option<crate::draft_discovery::Cache> = None;
                while let Ok(job) = requests.recv() {
                    let mut creation = None;
                    let mut saved_provider = None;
                    let (mut result, synced) = execute(
                        &job,
                        |step| {
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
                        *error = format!("Provider saved; subsequent action failed: {} / 供应商已保存，后续操作失败：{}",crate::i18n::message(error,false),crate::i18n::message(error,true));
                    }
                    let snapshot = if matches!(job.tag.as_str(), "metadata" | "workbuddy_check") {
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
                        if !matches!(job.tag.as_str(), "poll" | "metadata" | "workbuddy_check") {
                            if job.tag == "delete" && result.is_err() {
                                crate::config_refresh::existing_config(&dir)
                            } else {
                                Config::load(&dir).ok()
                            }
                        } else {
                            None
                        };
                    if results
                        .send(Reply {
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
