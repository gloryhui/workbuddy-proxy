#![allow(dead_code)]
mod journal { pub fn now_ms()->u64 {std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64} }
mod config {pub fn token()->String{format!("{:032x}",crate::journal::now_ms())}}
#[path="/Volumes/1TB/Downloads/git/workbuddy-proxy-usage-statistics/src/stats/mod.rs"]
mod stats;
use std::{sync::Arc,time::{Instant,Duration},fs};
use stats::{store::Writer,event::{Counters,StatsEvent,RequestMeta},dto::Source};
fn main(){
 let directory=std::env::temp_dir().join(format!("wbp-store-review-{}",std::process::id()));fs::create_dir_all(&directory).unwrap();
 let (mut writer,_)=Writer::open(&directory,"probe-run",Arc::new(Counters::default())).unwrap();
 writer.conn.busy_timeout(Duration::from_millis(30)).unwrap();
 let blocker=rusqlite::Connection::open(directory.join("usage.sqlite3")).unwrap();blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
 let batch:Vec<_>=(0..64).map(|i|StatsEvent::Start(Box::new(RequestMeta{request_id:format!("req-{i}"),run_id:"probe-run".into(),source:Source::Relay,provider_id:"provider".into(),provider_name:"Provider".into(),route_id:"route".into(),route_name:"Route".into(),requested_model:"model".into(),upstream_model:"model".into(),stream:true,started_at_ms:journal::now_ms() as i64}))).collect();
 let started=Instant::now();let result=writer.apply_batch(&batch);
 println!("{}",serde_json::json!({"batch_events":64,"busy_timeout_ms":30,"elapsed_ms":started.elapsed().as_millis(),"result_is_error":result.is_err()}));
 blocker.execute_batch("ROLLBACK").unwrap();drop(blocker);drop(writer);fs::remove_dir_all(directory).unwrap();
}
