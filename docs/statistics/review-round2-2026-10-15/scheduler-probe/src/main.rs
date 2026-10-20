#![allow(dead_code)]
#[path = "../../../../../gpui/src/stats_backend.rs"]
mod backend;
use backend::{JobKind,StatsScheduler};
use serde_json::json;
use std::{fs,net::TcpListener,io::{Read,Write},time::Duration,sync::{Arc,Mutex}};
fn main(){
 let directory=std::env::temp_dir().join(format!("wbp-scheduler-review-{}",std::process::id()));fs::create_dir_all(&directory).unwrap();
 let listener=TcpListener::bind("127.0.0.1:0").unwrap();let port=listener.local_addr().unwrap().port();
 fs::write(directory.join("instance.json"),json!({"port":port,"management_key":"fixture-only"}).to_string()).unwrap();
 let seen=Arc::new(Mutex::new(Vec::new()));let observed=seen.clone();
 std::thread::spawn(move||{for incoming in listener.incoming(){let mut socket=incoming.unwrap();socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();let mut bytes=Vec::new();let mut buffer=[0;4096];loop{let n=socket.read(&mut buffer).unwrap();if n==0{break}bytes.extend_from_slice(&buffer[..n]);if let Some(index)=bytes.windows(4).position(|x|x==b"\r\n\r\n"){let head=String::from_utf8_lossy(&bytes[..index]);let len=head.lines().find_map(|x|x.to_lowercase().strip_prefix("content-length:").map(|v|v.trim().parse::<usize>().unwrap())).unwrap();if bytes.len()>=index+4+len{let body:serde_json::Value=serde_json::from_slice(&bytes[index+4..index+4+len]).unwrap();observed.lock().unwrap().push(body["request_tag"].as_str().unwrap().to_string());break}}}
 let body=r#"{"message":"{}"}"#;write!(socket,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();}});
 let scheduler=StatsScheduler::new(directory.clone());
 scheduler.submit_read(JobKind::Filters,"filters-0".into(),json!({"command":"stats_filters","request_tag":"filters-0","kind":"provider"}));
 scheduler.submit_read(JobKind::Filters,"models-0".into(),json!({"command":"stats_filters","request_tag":"models-0","kind":"model"}));
 scheduler.pump();std::thread::sleep(Duration::from_millis(500));
 let replies:Vec<_>=scheduler.replies().try_iter().map(|r|r.tag).collect();
 println!("submitted provider and model filters before pump; dispatched={:?}; replies={:?}",seen.lock().unwrap(),replies);
 assert_eq!(replies,vec!["models-0"]); // Observed bug, not a passing product requirement.
 fs::remove_dir_all(directory).unwrap();
}
