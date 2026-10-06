#!/usr/bin/env python3
"""Run the actual async game-loop information request on one Tokio worker."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
source = (ROOT / 'src/connection_handler.rs').read_text()
helper = re.search(r'^async fn request_server_info\(.*?^}', source, re.S | re.M).group()
tests = r'''
#![allow(dead_code)]
use crossbeam_channel::{Sender, unbounded};
use tokio::sync::oneshot;
struct ServerInfoPacket { info:u32, time:f64 }
#[test] fn pending_reply_leaves_single_worker_available() {
    let runtime=tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
    runtime.block_on(async {
        let (sender,receiver)=unbounded();
        let game_loop=async {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            let reply:oneshot::Sender<ServerInfoPacket>=receiver.try_recv().unwrap();
            reply.send(ServerInfoPacket {info:42,time:7.0}).unwrap_or_else(|_|panic!("request abandoned"));
        };
        let (packet,())=tokio::join!(request_server_info(&sender),game_loop);
        let packet=packet.unwrap();assert_eq!(packet.info,42);assert_eq!(packet.time,7.0);
    });
}
#[test] fn closed_request_queue_is_reported() {
    let runtime=tokio::runtime::Builder::new_current_thread().build().unwrap();
    let (sender,receiver)=unbounded();drop(receiver);
    assert!(runtime.block_on(request_server_info(&sender)).is_err());
}
#[test] fn dropped_reply_is_reported_without_parking_worker() {
    let runtime=tokio::runtime::Builder::new_current_thread().build().unwrap();
    runtime.block_on(async {
        let (sender,receiver)=unbounded();
        let game_loop=async { tokio::task::yield_now().await;drop(receiver.try_recv().unwrap()); };
        let (result,())=tokio::join!(request_server_info(&sender),game_loop);
        assert!(result.is_err());
    });
}
'''
with tempfile.TemporaryDirectory(prefix='veloren-connection-info-') as directory:
    folder=Path(directory)
    (folder/'tests.rs').write_text(tests+'\n'+helper)
    (folder/'Cargo.toml').write_text('''[package]
name="veloren-connection-info-contract"
version="0.1.0"
edition="2024"
[lib]
path="tests.rs"
[dependencies]
crossbeam-channel="=0.5.15"
tokio={version="=1.52.3",features=["rt","time","sync","macros"]}
''')
    subprocess.run(['cargo','test','--offline','--manifest-path',str(folder/'Cargo.toml'),'--target','x86_64-unknown-linux-gnu','--target-dir','/tmp/veloren-connection-info-target','-q'],cwd=folder,check=True,timeout=120)
