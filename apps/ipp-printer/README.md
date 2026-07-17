# TRUEOS IPP printer Blueprint

`ipp-printer` is a userspace IPP Everywhere client for network printers. It
keeps printer policy and protocol parsing out of the kernel: TRUEOS supplies
Tokio-compatible TCP/UDP sockets, while the Blueprint owns DNS-SD discovery,
IPP capability negotiation, job attributes, and document submission.

The app uses the Rust `ipp` crate for the standards codec and the
TRUEOS-patched Tokio TCP/UDP stack for transport. No USB printer class, CUPS
daemon, vendor PPD, or kernel printer driver is needed.

## Commands

```text
ipp-printer discover [milliseconds]
ipp-printer info ipp://192.0.2.10:631/ipp/print
ipp-printer print ipp://192.0.2.10:631/ipp/print document.pdf
ipp-printer print auto photo.jpg --media iso_a4_210x297mm --quality high
ipp-printer print auto page.pwg --copies 2 --sides two-sided-long-edge
```

Discovery queries the standard `_ipp._tcp`, IPP Everywhere subtype, and
`_ipps._tcp` DNS-SD services over mDNS. Direct IP URIs remain available for
young kernels or networks that filter multicast.

Plain IPP is operational. Secure IPPS advertisements are shown by discovery,
and automatic selection prefers the operational `_ipp._tcp` service. Explicit
`ipps://` use currently returns a clear error until printer certificate trust
can be handled by the Blueprint TLS connector.

Before printing, the app asks the printer for `document-format-supported` and
refuses a format the printer did not advertise. Supported input is whatever
the selected printer reports; filename inference covers PDF, JPEG, PWG Raster,
Apple Raster, PostScript, PCL, and plain text. `--format` can supply any other
exact MIME type.

IPP Everywhere's baseline interchange format is PWG Raster. This first
Blueprint submits existing printable documents; rendering arbitrary TRUEOS
surfaces into PWG Raster is intentionally a separate graphics/spooler layer.
