//! Fixed public Release source. Cache only successful, validated stable tags.
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
const API: &str = "https://api.github.com/repos/gloryhui/workbuddy-proxy/releases/latest";
const PAGE: &str = "https://github.com/gloryhui/workbuddy-proxy/releases/tag/";
#[derive(Serialize, Deserialize)]
struct Cache {
    checked_at: u64,
    tag: String,
}
fn version(tag: &str) -> Option<[u32; 3]> {
    if tag.len() > 32 {
        return None;
    }
    let mut parts = tag.strip_prefix('v').unwrap_or(tag).split('.');
    let mut result = [0; 3];
    for part in &mut result {
        let s = parts.next()?;
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *part = s.parse().ok()?;
    }
    parts.next().is_none().then_some(result)
}
fn result(tag: &str, current: &str, cached: bool) -> Result<String, String> {
    let available = version(tag).ok_or("Invalid release version")?
        > version(current).ok_or("Invalid current version")?;
    Ok(serde_json::json!({"tag":tag,"current":current,"available":available,"url":format!("{PAGE}{tag}"),"cached":cached}).to_string())
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn cached(path: &Path) -> Option<Cache> {
    if std::fs::metadata(path).ok()?.len() > 4096 {
        return None;
    }
    let value: Cache = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    if now().checked_sub(value.checked_at)? >= 24 * 60 * 60 || version(&value.tag).is_none() {
        return None;
    }
    Some(value)
}
async fn network(client: &reqwest::Client, api: &str) -> Result<String, String> {
    let response = client
        .get(api)
        .header("Accept", "application/vnd.github+json")
        .header(
            "User-Agent",
            concat!("WorkBuddy-Proxy/", env!("CARGO_PKG_VERSION")),
        )
        .header("X-GitHub-Api-Version", "2026-03-10")
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .await
        .map_err(|_| "Update check failed; check network")?;
    if !response.status().is_success() {
        return Err(format!("Update check HTTP {}", response.status().as_u16()));
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let b = chunk.map_err(|_| "Invalid release response")?;
        if bytes.len() + b.len() > 128 * 1024 {
            return Err("Release response exceeds 128 KiB".into());
        }
        bytes.extend_from_slice(&b);
    }
    let data: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid release response")?;
    if data["draft"] != false || data["prerelease"] != false {
        return Err("Expected a stable release".into());
    }
    let tag = data["tag_name"].as_str().ok_or("Invalid release version")?;
    version(tag).ok_or("Invalid release version")?;
    Ok(tag.to_owned())
}
pub async fn check(client: &reqwest::Client, dir: &Path, force: bool) -> Result<String, String> {
    let path = dir.join("update-cache.json");
    if !force && let Some(cache) = cached(&path) {
        return result(&cache.tag, env!("CARGO_PKG_VERSION"), true);
    }
    let tag = tokio::time::timeout(std::time::Duration::from_secs(8), network(client, API))
        .await
        .map_err(|_| "Update check timed out")??;
    // Disk failure must not turn a valid network result into 'up to date' or hide an update.
    let _ = crate::config::write_json(
        &path,
        &Cache {
            checked_at: now(),
            tag: tag.clone(),
        },
    );
    result(&tag, env!("CARGO_PKG_VERSION"), false)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_versions_and_cache() {
        assert!(version("v0.10.0") > version("v0.9.9"));
        for tag in [
            "v1.0.0-beta",
            "v1.0",
            "../1.0.0",
            "v1.a.0",
            "v4294967296.0.0",
        ] {
            assert!(version(tag).is_none());
        }
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&result("v0.4.0", "0.4.0", true).unwrap())
                .unwrap()["available"],
            false
        );
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("update-cache.json");
        crate::config::write_json(
            &p,
            &Cache {
                tag: "v0.5.0".into(),
                checked_at: now(),
            },
        )
        .unwrap();
        assert!(cached(&p).is_some());
        crate::config::write_json(
            &p,
            &Cache {
                tag: "v0.5.0".into(),
                checked_at: now() - 86401,
            },
        )
        .unwrap();
        assert!(cached(&p).is_none());
    }
    #[tokio::test(flavor = "current_thread")]
    async fn validates_public_response_without_credentials() {
        use std::io::{Read, Write};
        for (body, ok) in [
            (
                r#"{"tag_name":"v0.5.0","draft":false,"prerelease":false}"#,
                true,
            ),
            (
                r#"{"tag_name":"v0.5.0-beta","draft":false,"prerelease":true}"#,
                false,
            ),
        ] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/latest", listener.local_addr().unwrap());
            let worker = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                let mut bytes = [0; 4096];
                let n = socket.read(&mut bytes).unwrap();
                let request = String::from_utf8_lossy(&bytes[..n]).to_ascii_lowercase();
                assert!(request.contains("user-agent: workbuddy-proxy/"));
                assert!(!request.contains("authorization:"));
                write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            });
            let client = reqwest::Client::builder().no_proxy().build().unwrap();
            assert_eq!(network(&client, &url).await.is_ok(), ok);
            worker.join().unwrap();
        }
    }
}
