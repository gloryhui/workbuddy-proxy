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
}
pub struct Backend {
    pub jobs: SyncSender<Job>,
    pub replies: Receiver<Reply>,
}
pub fn call(dir: &std::path::Path, payload: &Value) -> Result<String, String> {
    let info: Value = serde_json::from_slice(
        &fs::read(dir.join("instance.json")).map_err(|_| "Core is not responding")?,
    )
    .map_err(|_| "Invalid core response")?;
    let port = info["port"]
        .as_u64()
        .filter(|p| *p > 0 && *p <= 65535)
        .ok_or("Invalid core response")?;
    let key = info["management_key"]
        .as_str()
        .ok_or("Invalid core response")?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .proxy(None)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(
            if payload["command"] == "snapshot" {
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
        .map_err(|_| "Core is not responding")?;
    let success = response.status().is_success();
    let data: Value = response
        .body_mut()
        .with_config()
        .limit(1024 * 1024)
        .read_json()
        .map_err(|_| "Invalid core response")?;
    let message = data["message"]
        .as_str()
        .unwrap_or("Invalid core response")
        .to_owned();
    if success { Ok(message) } else { Err(message) }
}
impl Backend {
    pub fn new(dir: PathBuf) -> Self {
        let (jobs, requests) = mpsc::sync_channel::<Job>(8);
        let (results, replies) = mpsc::sync_channel(8);
        std::thread::Builder::new()
            .name("settings-control".into())
            .stack_size(512 * 1024)
            .spawn(move || {
                while let Ok(job) = requests.recv() {
                    let mut result = Ok(String::new());
                    for step in job.steps {
                        result = call(&dir, &step);
                        if result.is_err() {
                            break;
                        }
                    }
                    let snapshot = if job.tag == "metadata" {
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
                    let config = if job.tag != "poll" && job.tag != "metadata" {
                        Config::load(&dir).ok()
                    } else {
                        None
                    };
                    if results
                        .send(Reply {
                            tag: job.tag,
                            result,
                            config,
                            snapshot,
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
