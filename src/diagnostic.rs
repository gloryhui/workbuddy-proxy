//! Connection-test metadata only. Never include upstream bodies or headers.
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct TestReport {
    pub kind: String,
    pub provider: String,
    pub name: String,
    pub model: String,
    pub url: String,
    pub reasoning: String,
    pub http: String,
    pub headers_ms: Option<u64>,
    pub first_event_ms: Option<u64>,
    pub elapsed_ms: u64,
    pub response_model: Option<String>,
    pub reason: String,
}
impl TestReport {
    pub fn from_connection(c: &crate::config::Connection) -> Self {
        Self {
            kind: "connection_test".into(),
            provider: c.provider.clone(),
            name: c.name.clone(),
            model: c.model.clone(),
            url: c.endpoint(),
            reasoning: c.reasoning_effort.clone(),
            http: String::new(),
            headers_ms: None,
            first_event_ms: None,
            elapsed_ms: 0,
            response_model: None,
            reason: String::new(),
        }
    }
    pub fn json(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
    pub fn render(&self, zh: bool) -> String {
        let reason = crate::i18n::message(&self.reason, zh);
        let mut rows = if zh {
            vec![
                if self.reason.is_empty() {
                    "测试连接成功".into()
                } else {
                    format!("测试连接失败：{reason}")
                },
                format!("供应商：{}", self.provider),
                format!("模型：{}（{}）", self.name, self.model),
                format!("请求：POST {}", self.url),
            ]
        } else {
            vec![
                if self.reason.is_empty() {
                    "Connection test passed".into()
                } else {
                    format!("Connection test failed: {reason}")
                },
                format!("Provider: {}", self.provider),
                format!("Model: {} ({})", self.name, self.model),
                format!("Request: POST {}", self.url),
            ]
        };
        let effort = match self.reasoning.as_str() {
            "default" => {
                if zh {
                    "跟随 WorkBuddy（本次不覆盖）"
                } else {
                    "Follow WorkBuddy (no override in this test)"
                }
            }
            "none" => {
                if zh {
                    "关闭"
                } else {
                    "Off"
                }
            }
            "low" => {
                if zh {
                    "低"
                } else {
                    "Low"
                }
            }
            "high" => {
                if zh {
                    "高"
                } else {
                    "High"
                }
            }
            "max" => {
                if zh {
                    "最高"
                } else {
                    "Max"
                }
            }
            _ => "—",
        };
        rows.push(if zh {
            format!("思考设置：{effort}；本次输出上限：1 token")
        } else {
            format!("Thinking: {effort}; test output limit: 1 token")
        });
        if !self.http.is_empty() {
            rows.push(self.http.clone());
        }
        if let Some(ms) = self.headers_ms {
            rows.push(if zh {
                format!("收到响应头：{ms} ms")
            } else {
                format!("Response headers: {ms} ms")
            });
        }
        if let Some(ms) = self.first_event_ms {
            rows.push(if zh {
                format!("首个有效 SSE 事件：{ms} ms")
            } else {
                format!("First valid SSE event: {ms} ms")
            });
        }
        rows.push(if zh {
            format!("测试总耗时：{} ms", self.elapsed_ms)
        } else {
            format!("Total test time: {} ms", self.elapsed_ms)
        });
        if let Some(model) = &self.response_model {
            rows.push(if zh {
                format!("上游返回模型：{model}")
            } else {
                format!("Upstream model: {model}")
            });
        }
        if self.reason.is_empty() {
            rows.push(if zh { "流式校验通过；收到首个有效事件后结束测试，未等待完整生成。" } else { "Streaming validated; test ended at the first valid event, without waiting for full generation." }.into());
        } else if !self.http.is_empty() {
            let hint = match self.http.as_str() {
                "HTTP 401" => {
                    if zh {
                        "检查 API Key 是否正确、有效。"
                    } else {
                        "Check whether the API key is valid."
                    }
                }
                "HTTP 403" => {
                    if zh {
                        "检查账号权限或订阅是否支持该模型。"
                    } else {
                        "Check account permissions and model access."
                    }
                }
                "HTTP 404" => {
                    if zh {
                        "检查 API 地址和模型 ID。"
                    } else {
                        "Check the API URL and model ID."
                    }
                }
                "HTTP 429" => {
                    if zh {
                        "检查额度、订阅限额或请求频率。"
                    } else {
                        "Check quota, subscription limits and request rate."
                    }
                }
                _ => {
                    if zh {
                        "检查供应商状态、网络和代理设置。"
                    } else {
                        "Check provider status, network and proxy settings."
                    }
                }
            };
            rows.push(hint.into());
        }
        rows.join("\n")
    }
}
