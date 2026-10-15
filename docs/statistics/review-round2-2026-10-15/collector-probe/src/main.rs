#![allow(dead_code)]
mod stats {
    #[path = "/Volumes/1TB/Downloads/git/workbuddy-proxy-usage-statistics/src/stats/dto.rs"]
    pub mod dto;
    #[path = "/Volumes/1TB/Downloads/git/workbuddy-proxy-usage-statistics/src/stats/collector.rs"]
    pub mod collector;
}
use stats::collector::{Collector,Protocol,UsageAccumulator};
use stats::dto::UsageReason;
use serde_json::json;
fn main() {
    let mut stream=b"data: {\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":100,\"total_tokens\":1100}}\n\n".to_vec();
    for _ in 0..15000 { stream.extend_from_slice(b": heartbeat\n\n"); }
    stream.extend_from_slice(b"data: [DONE]\n\n");
    let mut one=Collector::new(Protocol::Sse,false);one.observe(&stream);
    let mut split=Collector::new(Protocol::Sse,false);for part in stream.chunks(4096){split.observe(part)}
    println!("same valid SSE {} bytes: one_chunk={:?}, split_chunks={:?}",stream.len(),one.finish(UsageReason::NoUsage).status,split.finish(UsageReason::NoUsage).status);
    let mut usage=UsageAccumulator::default();
    usage.observe(&json!({"prompt_tokens":100}));
    usage.observe(&json!({"prompt_tokens":200,"completion_tokens":10,"total_tokens":210}));
    println!("conflicting partial merged: {:?}",usage.finalize(UsageReason::NoUsage));
    let mut fields=UsageAccumulator::default();
    fields.observe(&json!({"prompt_tokens":100,"completion_tokens":10,"total_tokens":110}));
    fields.observe(&json!({"prompt_tokens":100,"completion_tokens":10,"total_tokens":110,"prompt_tokens_details":{"cached_tokens":80}}));
    println!("same totals adding valid cache detail: {:?}",fields.finalize(UsageReason::NoUsage));
}
