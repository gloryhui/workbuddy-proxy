//! Native 上游协议适配子系统的公共入口。Chat Completions 继续走原始字节代理路径；
//! Responses、Anthropic Messages 和 Gemini Generate Content 才在这里解析、编码和
//! 反解。
//!
//! 上下游关系：
//!
//! - 上游调用方是 `proxy::native`。它以 [`request::ChatRequest::parse`] 统一接收客户端
//!   Chat 语义，用 [`replay`] 补充 opaque 历史，再调用 [`encode::encode`] 生成供应商
//!   请求；响应侧由 `output`、`streaming` 和 `sse` 转回 Chat 语义。
//! - 下游没有独立线程，也不直接访问网络或 SQLite。这些子模块是同步、可测试的数据
//!   变换；HTTP、超时、取消、统计 guard 和并发 permit 都由 `proxy::native` 持有。
//!
//! 数据所有权上，解析结果拥有自己的字符串和 JSON 值；编码函数只借用
//! `ChatRequest`、配置及 replay 映射，返回新的 `serde_json::Value`。HTTP 请求体与
//! 响应流仍归 `proxy::native` 管理，因此这里的函数返回并不表示网络成功，更不表示
//! 客户端已经收到结果。
//!
//! `config` 不是本目录里的第二个独立配置模块。[`crate::main`] 通过
//! `#[path = "protocol/config.rs"]` 把同一文件编译成 `crate::upstream_config`，这里
//! 再以 `protocol::config` 名称重导出；在本 core crate 内两条路径指向同一类型。
//! GPUI 是独立 crate/进程，它会从同一源码路径独立编译自己的类型和内存实例。
pub use crate::upstream_config as config;
pub mod encode;
pub mod output;
pub mod replay;
pub mod request;
pub mod sse;
pub mod streaming;
pub mod usage;

/// native 转换与上游协议解析共用的结构化错误。
///
/// `code` 是稳定的机器可读分类，用于 HTTP envelope 和统计终态判断；`param` 只记录
/// JSON 结构路径或字段名，不能放客户端原文、prompt、工具参数或凭据；`status` 是
/// 建议返回给本地 Chat 客户端的 HTTP 状态码。
///
/// [`Error::input`] 生成的请求/能力问题通常为 400，[`Error::upstream`] 生成的上游
/// 响应或连接问题通常为 502。`conversation_state_expired` 等 replay 分支会直接构造
/// 409。这里只构造错误，不发送响应；`proxy::native::failure` 才把状态和 envelope
/// 写回 HTTP。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub code: &'static str,
    pub param: Option<String>,
    pub status: u16,
}

impl Error {
    pub fn input(code: &'static str, path: impl Into<String>) -> Self {
        Self {
            code,
            param: Some(path.into()),
            status: 400,
        }
    }

    pub fn upstream(code: &'static str) -> Self {
        Self {
            code,
            param: None,
            status: 502,
        }
    }

    pub fn envelope(&self) -> serde_json::Value {
        // envelope 的消息固定为分类文案，不能回显 `param` 之外的用户输入。`param`
        // 自身也可能指出具体字段，但构造点上必须只传结构路径，不能拼接字段值。
        serde_json::json!({"error":{
            "message": match self.status {
                _ if self.code=="upstream_http_error"=>"Upstream rejected the request",
                _ if self.code=="upstream_connection_error"=>"Upstream connection failed",
                _ if self.code=="upstream_timeout"=>"Upstream response timed out",
                409 => "Conversation state expired; start a new conversation",
                400 => "Request cannot be represented by the selected upstream protocol",
                _ => "Upstream protocol response was invalid or incomplete",
            },
            "type": if self.status == 400 && self.param.is_some() { "invalid_request_error" } else { "upstream_error" },
            "param":self.param,"code":self.code
        }})
    }
}
