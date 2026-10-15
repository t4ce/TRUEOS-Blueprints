//! Bounded error categories only: never return error text, URLs or response data.
use std::error::Error;
use std::io;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct SourceSummary {
    pub depth: usize,
    pub truncated: bool,
    pub io_kind: &'static str,
    pub tls_class: &'static str,
    pub classes: String,
}

fn io_class(kind: io::ErrorKind) -> &'static str {
    match kind {
        io::ErrorKind::TimedOut => "io_timed_out",
        io::ErrorKind::ConnectionRefused => "io_connection_refused",
        io::ErrorKind::ConnectionReset => "io_connection_reset",
        io::ErrorKind::ConnectionAborted => "io_connection_aborted",
        io::ErrorKind::NotConnected => "io_not_connected",
        io::ErrorKind::AddrNotAvailable => "io_address_unavailable",
        io::ErrorKind::NetworkUnreachable => "io_network_unreachable",
        io::ErrorKind::HostUnreachable => "io_host_unreachable",
        io::ErrorKind::BrokenPipe => "io_broken_pipe",
        io::ErrorKind::UnexpectedEof => "io_unexpected_eof",
        io::ErrorKind::InvalidData => "io_invalid_data",
        io::ErrorKind::WouldBlock => "io_would_block",
        io::ErrorKind::PermissionDenied => "io_permission_denied",
        _ => "io_other",
    }
}

pub(super) fn summarize(error: &(dyn Error + 'static)) -> SourceSummary {
    let mut result = SourceSummary {
        depth: 0,
        truncated: false,
        io_kind: "none",
        tls_class: "none",
        classes: String::new(),
    };
    let mut source = Some(error);
    while let Some(error) = source {
        if result.depth == 16 {
            result.truncated = true;
            break;
        }
        // Formatting stays in memory. Only exact known TLS error variants may
        // affect output; arbitrary source text is never copied into a record.
        let diagnostic = format!("{error:?}");
        let tls = if diagnostic.contains("InvalidCertificate(UnknownIssuer)")
            || diagnostic.contains("invalid peer certificate: UnknownIssuer")
        {
            "tls_unknown_issuer"
        } else if diagnostic.contains("InvalidCertificate(NotValidYet")
            || diagnostic.contains("invalid peer certificate: NotValidYet")
        {
            "tls_not_valid_yet"
        } else if diagnostic.contains("InvalidCertificate(Expired")
            || diagnostic.contains("invalid peer certificate: Expired")
        {
            "tls_expired"
        } else if diagnostic.contains("InvalidCertificate(NotValidForName")
            || diagnostic.contains("invalid peer certificate: NotValidForName")
        {
            "tls_name_mismatch"
        } else if diagnostic.contains("InvalidCertificate(")
            || diagnostic.contains("invalid peer certificate:")
        {
            "tls_certificate_other"
        } else {
            "none"
        };
        if tls != "none" {
            result.tls_class = tls;
        }
        let class = if let Some(error) = error.downcast_ref::<io::Error>() {
            result.io_kind = io_class(error.kind());
            result.io_kind
        } else if tls != "none" {
            tls
        } else {
            "opaque"
        };
        if !result.classes.is_empty() {
            result.classes.push(',');
        }
        result.classes.push_str(class);
        result.depth += 1;
        source = error.source();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Debug)]
    struct Wrapper(&'static str, Box<dyn Error + Send + Sync>);
    impl std::fmt::Display for Wrapper {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.0)
        }
    }
    impl Error for Wrapper {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(self.1.as_ref())
        }
    }
    #[test]
    fn preserves_typed_io_sources_without_private_error_text() {
        let error = Wrapper(
            "https://private.example/token=secret",
            Box::new(io::Error::from(io::ErrorKind::ConnectionReset)),
        );
        let summary = summarize(&error);
        assert_eq!(summary.depth, 2);
        assert_eq!(summary.io_kind, "io_connection_reset");
        assert_eq!(summary.classes, "opaque,io_connection_reset");
        assert!(!format!("{summary:?}").contains("private"));
        assert!(!format!("{summary:?}").contains("secret"));
    }
    #[test]
    fn tls_error_variants_remain_static_and_timeouts_remain_typed() {
        let error = io::Error::other("invalid peer certificate: UnknownIssuer");
        assert_eq!(summarize(&error).tls_class, "tls_unknown_issuer");
        assert_eq!(
            summarize(&io::Error::from(io::ErrorKind::TimedOut)).io_kind,
            "io_timed_out"
        );
    }
    #[derive(Debug)]
    struct Cycle;
    impl std::fmt::Display for Cycle {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("cycle")
        }
    }
    impl Error for Cycle {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(self)
        }
    }
    #[test]
    fn cyclic_sources_are_bounded() {
        let summary = summarize(&Cycle);
        assert_eq!(summary.depth, 16);
        assert!(summary.truncated);
    }
}
