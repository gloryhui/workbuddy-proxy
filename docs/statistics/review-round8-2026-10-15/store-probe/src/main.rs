#![allow(dead_code)]
mod journal { pub fn now_ms()->u64 {std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64} }
mod config {pub fn token()->String{format!("{:032x}",crate::journal::now_ms())}}
#[path="../../../../../src/stats/mod.rs"]
mod stats;
use std::{sync::Arc,fs};
use stats::{store::{Writer,RecoveryScan,scan_recovery_batch},event::Counters};
fn main(){
 let directory=std::env::temp_dir().join(format!("wbp-r8-scan-{}",std::process::id()));fs::create_dir_all(&directory).unwrap();
 let (mut writer,_)=Writer::open(&directory,"probe-run",Arc::new(Counters::default())).unwrap();
 writer.conn.execute_batch("INSERT INTO usage_requests(request_id,run_id,source,provider_id,route_id,provider_name,route_name,requested_model,upstream_model,started_at_ms,stream,outcome,ended_at_ms,duration_ms,usage_status,usage_source,created_at_ms,updated_at_ms) VALUES('pending-1','probe-run','relay','p','r','P','R','m','m',1000,1,'interrupted',1000,0,'missing','none',1000,1000);
 INSERT INTO usage_costs(request_id,status,currency,price_source,input_mode,input_rate_nanos,output_rate_nanos,cache_read_rate_nanos,cache_write_rate_nanos,price_snapshot_at_ms,pricing_version,calculated_at_ms) VALUES('pending-1','pending','CNY','default_zero','flat',0,0,0,0,1000,2,1000);
 CREATE TRIGGER fail_one_settlement BEFORE UPDATE ON usage_costs WHEN OLD.request_id='pending-1' BEGIN SELECT RAISE(ABORT,'review transient settlement refusal'); END;").unwrap();
 let as_of=writer.conn.query_row("SELECT MAX(id) FROM usage_requests",[],|r|r.get(0)).unwrap();
 let mut scan=RecoveryScan::new(as_of);
 let first=scan_recovery_batch(&mut writer,&mut scan,256,2000).unwrap();
 writer.conn.execute_batch("DROP TRIGGER fail_one_settlement").unwrap();
 let second=scan_recovery_batch(&mut writer,&mut scan,256,3000).unwrap();
 let pending:i64=writer.conn.query_row("SELECT COUNT(*) FROM usage_costs WHERE status='pending'",[],|r|r.get(0)).unwrap();
 let backlog=stats::store::backlog_len(&writer.conn).unwrap();
 println!("{}",serde_json::json!({"first":first,"second_after_failure_removed":second,"scan_finished":scan.finished(),"pending":pending,"backlog":backlog}));
 assert!(scan.finished() && pending==1 && backlog==0,"expected reproduction changed");
 drop(writer);fs::remove_dir_all(directory).unwrap();
}
