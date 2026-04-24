extern crate alloc;

use socket2::{Domain, Protocol, Socket, Type};
use trueos_blueprint::bp_info;

fn main() {
    bp_info!("socket2_probe: start");

    match Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP)) {
        Ok(_) => bp_info!("socket2_probe: unexpected-ok"),
        Err(err) => {
            let line = alloc::format!("socket2_probe: err kind={:?} msg={}", err.kind(), err);
            bp_info!(line.as_str());
        }
    }

    bp_info!("socket2_probe: done");
}
