//! Delegates to TRUEOS's secure system DNS service. No DNS transport lives here.
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr};

use rama_net::address::Domain;

use crate::DnsResolver;

/// The system's DoH/DoT resolver, exposed through its existing IPv4 ABI.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TrueosDns;

impl DnsResolver for TrueosDns {
    type Error = io::Error;

    async fn ipv4_lookup(&self, domain: Domain) -> io::Result<Vec<Ipv4Addr>> {
        lookup_ipv4_with(domain, resolve_ipv4).await
    }

    async fn ipv6_lookup(&self, _domain: Domain) -> io::Result<Vec<Ipv6Addr>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "TRUEOS's secure DNS ABI does not expose IPv6 lookups",
        ))
    }

    async fn txt_lookup(&self, _domain: Domain) -> io::Result<Vec<Vec<u8>>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "TRUEOS's secure DNS ABI does not expose TXT lookups",
        ))
    }
}

async fn lookup_ipv4_with(
    domain: Domain,
    resolve: impl FnOnce(&str) -> io::Result<Ipv4Addr> + Send + 'static,
) -> io::Result<Vec<Ipv4Addr>> {
    // The synchronous ABI parks its caller while the kernel's broker resolves
    // the request. Keep that work off the async executor's polling lane.
    tokio::task::spawn_blocking(move || resolve(domain.as_str()).map(|ip| vec![ip]))
        .await
        .map_err(io::Error::other)?
}

#[cfg(target_os = "trueos")]
fn resolve_ipv4(host: &str) -> io::Result<Ipv4Addr> {
    unsafe extern "C" {
        fn trueos_cabi_dns_resolve_ipv4(
            host: *const u8,
            host_len: usize,
            out_octets: *mut u8,
        ) -> i32;
    }
    let mut octets = [0u8; 4];
    // The name and writable four-byte output remain valid for the entire call.
    let status =
        unsafe { trueos_cabi_dns_resolve_ipv4(host.as_ptr(), host.len(), octets.as_mut_ptr()) };
    if status == 0 {
        Ok(Ipv4Addr::from(octets))
    } else {
        Err(io::Error::from_raw_os_error(status))
    }
}

#[cfg(all(test, not(target_os = "trueos")))]
fn resolve_ipv4(_host: &str) -> io::Result<Ipv4Addr> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "the TRUEOS DNS ABI is unavailable on this host",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn delegates_name_and_returns_system_address() {
        let poll_thread = std::thread::current().id();
        let addresses = lookup_ipv4_with("service.invalid".parse().unwrap(), move |name| {
            assert_ne!(std::thread::current().id(), poll_thread);
            assert_eq!(name, "service.invalid");
            Ok(Ipv4Addr::new(192, 0, 2, 1))
        })
        .await
        .unwrap();
        assert_eq!(addresses, [Ipv4Addr::new(192, 0, 2, 1)]);
    }

    #[tokio::test]
    async fn preserves_system_failure() {
        let error = lookup_ipv4_with("service.invalid".parse().unwrap(), |_| {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "blocked"))
        })
        .await
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(error.to_string(), "blocked");
    }

    #[tokio::test]
    async fn unsupported_records_do_not_use_another_resolver() {
        let domain: Domain = "service.invalid".parse().unwrap();
        assert_eq!(
            TrueosDns
                .ipv6_lookup(domain.clone())
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::Unsupported
        );
        assert_eq!(
            TrueosDns.txt_lookup(domain).await.unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
    }
}
