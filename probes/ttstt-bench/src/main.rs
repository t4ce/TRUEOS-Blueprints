//! Finite native loopback benchmark against an already-running ttstt service.
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};
fn notice(message: String) {
    #[cfg(target_os = "trueos")]
    {
        trueos::logl::log(trueos::logl::level::INFO, message.as_str());
        let _ = trueos::logl::log_record(trueos::logl::level::IMPORTANT, "blueprint/ttstt-bench", message.as_str());
    }
    #[cfg(not(target_os = "trueos"))]
    println!("{message}");
}
fn request(method: &str, path: &str, body: &[u8]) -> Result<(), String> {
    let mut stream = TcpStream::connect_timeout(&"127.0.0.1:9772".parse().unwrap(), Duration::from_secs(5)).map_err(|e| e.to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(210))).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;
    write!(stream,"{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).map_err(|e|e.to_string())?;
    stream.write_all(body).map_err(|e|e.to_string())?;
    let mut reply=String::new();
    stream.take(8192).read_to_string(&mut reply).map_err(|e|e.to_string())?;
    if !reply.starts_with("HTTP/1.1 200 ") { return Err(reply); }
    let (_,body)=reply.split_once("\r\n\r\n").ok_or("missing response body")?;
    let response:serde_json::Value=serde_json::from_str(body).map_err(|e|e.to_string())?;
    if path=="/health" {
        if response["ready"]!=true || response["protocol"]!="ttstt-v1" { return Err("TTS is not ready".into()); }
    } else if response["ok"]!=true { return Err("speech failed".into()); }
    Ok(())
}
fn run()->Result<(),String> {
    request("GET","/health",&[])?;
    let cases:Vec<(String,String)>=serde_json::from_str(include_str!("../perf_texts.json")).map_err(|e|e.to_string())?;
    for (name,text) in cases {
        notice(format!("ttstt-bench: case={name} submitted bytes={}",text.len()));
        let started=Instant::now();
        let body=serde_json::to_vec(&serde_json::json!({"text":text})).map_err(|e|e.to_string())?;
        request("POST","/speech",&body)?;
        notice(format!("ttstt-bench: case={name} done wall_ms={}",started.elapsed().as_millis()));
    }
    notice("ttstt-bench: all cases passed".into());
    Ok(())
}
fn main(){if let Err(error)=run(){notice(format!("ttstt-bench: FAILED {error}"));std::process::exit(1);}}
