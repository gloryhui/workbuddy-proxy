//! 桌面端检查 GitHub 正式 Release 的入口；仅写检查缓存，不下载安装包。
//!
//! 本模块只回答“最新正式版本是什么、当前平台是否有可在线安装的 asset、网页链接
//! 是什么”，不下载安装包。真正的下载、SHA-256 校验、暂存和安装分别由
//! `crate::updater::Updater` 的 `start`/`apply` 命令处理。这个分层让“检查更新”
//! 失败时不会触碰 updater 的下载会话或安装状态。
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
const API: &str = "https://api.github.com/repos/gloryhui/workbuddy-proxy/releases/latest";
const PAGE: &str = "https://github.com/gloryhui/workbuddy-proxy/releases/tag/";
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Release {
    /// 形如 `v1.2.3` 的纯数字三段版本；draft/prerelease 已在解析时排除。
    pub tag_name: String,
    /// 以下说明字段只用于界面展示，缺失时由 `serde(default)` 变成空值。
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub published_at: String,
    #[serde(default)]
    pub assets: Vec<serde_json::Value>,
}
#[derive(Serialize, Deserialize)]
struct Cache {
    /// Unix epoch 秒；只复用不在未来且年龄严格小于 24 小时的缓存。
    checked_at: u64,
    /// 旧格式缓存可能没有完整 release，但仍保留已校验的 tag 用作兼容读取。
    tag: String,
    #[serde(default)]
    release: Option<Release>,
}
fn version(tag: &str) -> Option<[u32; 3]> {
    // 这里接受可选的 `v` 前缀，但只接受恰好三段十进制 `u32`。因此预发布后缀、
    // 路径片段和溢出数字都返回 None；它不是完整的 SemVer 解析器。
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
    // 比较按三段数字而不是字符串字典序，避免 `0.10.0` 被误判为小于 `0.9.9`。
    // 返回 JSON 是给管理命令/UI 的快照，不是 updater 已经接受下载的证据。
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
    // 缓存先限制文件大小，再完整 JSON 反序列化；过期、未来时间或非法 tag 都当作
    // 未命中。`force=true` 会绕过这里。磁盘上的普通文件不是签名凭证，若用户修改
    // 了缓存，本函数只能保证结构、年龄和版本格式，不能证明内容必来自 GitHub。
    if std::fs::metadata(path).ok()?.len() > 128 * 1024 {
        return None;
    }
    let value: Cache = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    if now().checked_sub(value.checked_at)? >= 24 * 60 * 60 || version(&value.tag).is_none() {
        return None;
    }
    Some(value)
}
async fn network(client: &reqwest::Client, api: &str) -> Result<Release, String> {
    // 请求固定 GitHub API 地址，并明确带上 API accept/version 与 User-Agent；
    // 不附加 Authorization 头。响应正文边流式读取边限制为 128 KiB，避免把无界
    // 响应先完整放进内存。只有 `draft=false`、`prerelease=false` 和合法三段版本
    // 才形成 Release；这里不检查安装包 asset，asset 可用性由
    // `updater::availability` 基于同一次响应继续验证。
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
    let release: Release = serde_json::from_value(data).map_err(|_| "Invalid release response")?;
    if release.name.len() > 512 || release.body.len() > 64 * 1024 || release.published_at.len() > 64
    {
        return Err("Invalid release response".into());
    }
    Ok(release)
}
pub async fn check(client: &reqwest::Client, dir: &Path, force: bool) -> Result<String, String> {
    // 主流程分为两个互斥来源：
    //
    // 1. 非 force 且 24 小时缓存命中：不再访问网络，可能只还原旧缓存里的 tag；
    // 2. 其余情况：调用 `network`，成功后才尽力写回缓存，写缓存失败不会把已成功
    //    的网络检查改成失败。
    //
    // 随后把运行版本、网页 URL、平台和 `availability` 结果附加到响应。UI 当前发送
    // `force=true`，所以正常点击“检查更新”会绕过缓存；本函数仍不下载 asset。
    let path = dir.join("update-cache.json");
    let (release, from_cache) = if !force && let Some(cache) = cached(&path) {
        (
            cache.release.unwrap_or(Release {
                tag_name: cache.tag,
                ..Default::default()
            }),
            true,
        )
    } else {
        let release = tokio::time::timeout(std::time::Duration::from_secs(8), network(client, API))
            .await
            .map_err(|_| "Update check timed out")??;
        let _ = crate::config::write_json(
            &path,
            &Cache {
                checked_at: now(),
                tag: release.tag_name.clone(),
                release: Some(release.clone()),
            },
        );
        (release, false)
    };
    let mut value: serde_json::Value = serde_json::from_str(&result(
        &release.tag_name,
        env!("CARGO_PKG_VERSION"),
        from_cache,
    )?)
    .map_err(|_| "Invalid release response")?;
    value["name"] = serde_json::json!(release.name);
    value["body"] = serde_json::json!(release.body);
    value["published_at"] = serde_json::json!(release.published_at);
    value["platform"] = serde_json::json!(format!(
        "{} / {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    let support = crate::updater::availability(
        &serde_json::json!({"tag_name":release.tag_name,"draft":false,"prerelease":false,"assets":release.assets}),
    );
    value["online_supported"] = serde_json::json!(support.is_ok());
    value["online_error"] = serde_json::json!(support.err());
    Ok(value.to_string())
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
                release: None,
            },
        )
        .unwrap();
        assert!(cached(&p).is_some());
        crate::config::write_json(
            &p,
            &Cache {
                tag: "v0.5.0".into(),
                checked_at: now() - 86401,
                release: None,
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
