#![allow(dead_code)]
mod journal { pub fn now_ms()->u64 {std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64} }
mod config {pub fn token()->String{format!("{:032x}",crate::journal::now_ms())}}
#[path="../../../../../src/stats/mod.rs"]
mod stats;
use std::{sync::Arc,fs};
use stats::{store::{Writer,WriteBudget,BATCH_BUDGET,PriceSnapshot,Maintenance},event::{Counters,StatsEvent,RequestMeta},dto::{Source,MaintenanceKind}};
fn seed(writer:&mut Writer,n:i64){
 let events:Vec<_>=(0..n).map(|i|StatsEvent::Start(Box::new(RequestMeta{request_id:format!("req-{i}"),run_id:"probe-run".into(),source:Source::Relay,provider_id:"provider".into(),provider_name:"Provider".into(),route_id:"route".into(),route_name:"Route".into(),requested_model:"model".into(),upstream_model:"model".into(),stream:true,started_at_ms:1000+i,price:PriceSnapshot::default_zero("CNY",1000+i)}))).collect();
 writer.apply_batch(&events,&WriteBudget::new(BATCH_BUDGET)).unwrap();
 writer.conn.execute("UPDATE usage_requests SET outcome='completed',ended_at_ms=started_at_ms,updated_at_ms=started_at_ms,usage_status='missing'",[]).unwrap();
}
fn count(writer:&Writer)->i64{writer.conn.query_row("SELECT COUNT(*) FROM usage_requests",[],|r|r.get(0)).unwrap()}
fn main(){
 let directory=std::env::temp_dir().join(format!("wbp-store-review3-{}",std::process::id()));fs::create_dir_all(&directory).unwrap();
 let (mut writer,_)=Writer::open(&directory,"probe-run",Arc::new(Counters::default())).unwrap();
 seed(&mut writer,1200);
 let id=writer.insert_maintenance(MaintenanceKind::Clear,journal::now_ms() as i64).unwrap();
 writer.maintenance=Some(Maintenance::new(id,MaintenanceKind::Clear,1600,1200,None,journal::now_ms() as i64,500));
 let expected_eligible:i64=writer.conn.query_row("SELECT COUNT(*) FROM usage_requests WHERE started_at_ms<1600",[],|r|r.get(0)).unwrap();
 let mut steps=0;loop{steps+=1;if !writer.maintenance_step().unwrap(){break}}
 let old_left:i64=writer.conn.query_row("SELECT COUNT(*) FROM usage_requests WHERE started_at_ms<1600",[],|r|r.get(0)).unwrap();
 let deleted_new:i64=600-writer.conn.query_row::<i64,_,_>("SELECT COUNT(*) FROM usage_requests WHERE started_at_ms>=1600",[],|r|r.get(0)).unwrap();
 println!("{}",serde_json::json!({"case":"clear_multiple_batches","expected_eligible":expected_eligible,"deleted":1200-count(&writer),"eligible_left":old_left,"outside_preview_deleted":deleted_new,"steps":steps}));
 writer.conn.execute("DELETE FROM usage_requests",[]).unwrap();seed(&mut writer,1200);
 let as_of:i64=writer.conn.query_row("SELECT MAX(id) FROM usage_requests",[],|r|r.get(0)).unwrap();
 let id=writer.insert_maintenance(MaintenanceKind::Retention,journal::now_ms() as i64).unwrap();
 writer.maintenance=Some(Maintenance::new(id,MaintenanceKind::Retention,5000,as_of,None,journal::now_ms() as i64,500));
 let mut steps=0;loop{steps+=1;if !writer.maintenance_step().unwrap(){break}}
 println!("{}",serde_json::json!({"case":"retention_multiple_batches","expected_delete":1200,"deleted":1200-count(&writer),"left":count(&writer),"steps":steps}));
 drop(writer);fs::remove_dir_all(directory).unwrap();
}
