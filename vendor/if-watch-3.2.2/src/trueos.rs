use crate::{IfEvent, IpNet, Ipv4Net, Ipv6Net};
use futures::stream::{FusedStream, Stream};
use std::collections::{HashSet, VecDeque};
use std::io::Result;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::pin::Pin;
use std::task::{Context, Poll};

#[cfg(feature = "tokio")]
pub mod tokio {
    //! TRUEOS interface watcher for Tokio runtimes.

    /// Watches the addresses currently exposed by TRUEOS.
    pub type IfWatcher = super::IfWatcher;
}

#[cfg(feature = "smol")]
pub mod smol {
    //! TRUEOS interface watcher for smol runtimes.

    /// Watches the addresses currently exposed by TRUEOS.
    pub type IfWatcher = super::IfWatcher;
}

/// TRUEOS address watcher.
///
/// TRUEOS does not expose a userspace interface-change feed yet. Seed the
/// stable loopback networks and remain pending after their initial `Up`
/// events. Wildcard transport listeners continue to work normally.
#[derive(Debug)]
pub struct IfWatcher {
    addrs: HashSet<IpNet>,
    queue: VecDeque<IfEvent>,
}

impl IfWatcher {
    /// Create a watcher seeded with the IPv4 and IPv6 loopback networks.
    pub fn new() -> Result<Self> {
        let ipv4 = IpNet::V4(Ipv4Net::new(Ipv4Addr::LOCALHOST, 8).expect("valid IPv4 prefix"));
        let ipv6 = IpNet::V6(Ipv6Net::new(Ipv6Addr::LOCALHOST, 128).expect("valid IPv6 prefix"));
        let addrs = HashSet::from([ipv4, ipv6]);
        let queue = VecDeque::from([IfEvent::Up(ipv4), IfEvent::Up(ipv6)]);
        Ok(Self { addrs, queue })
    }

    /// Iterate over the currently known networks.
    pub fn iter(&self) -> impl Iterator<Item = &IpNet> {
        self.addrs.iter()
    }

    /// Poll for an address change event.
    pub fn poll_if_event(&mut self, _cx: &mut Context<'_>) -> Poll<Result<IfEvent>> {
        match self.queue.pop_front() {
            Some(event) => Poll::Ready(Ok(event)),
            None => Poll::Pending,
        }
    }
}

impl Stream for IfWatcher {
    type Item = Result<IfEvent>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::into_inner(self).poll_if_event(cx).map(Some)
    }
}

impl FusedStream for IfWatcher {
    fn is_terminated(&self) -> bool {
        false
    }
}
