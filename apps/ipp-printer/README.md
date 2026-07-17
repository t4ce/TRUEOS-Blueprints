# TRUEOS IPP printer Blueprint

`ipp-printer` is a userspace IPP Everywhere client for network printers. It
keeps print-job policy out of the kernel: the BSP-resident TRUEOS printer
service owns DNS-SD discovery and refreshes its registry every 15 seconds,
while the Blueprint owns IPP capability negotiation, job attributes, and
document submission over TRUEOS Tokio sockets.

The app uses the Rust `ipp` crate for the standards codec and the
TRUEOS-patched Tokio TCP stack for job transport. The kernel service owns mDNS
UDP discovery. No USB printer class, CUPS daemon, vendor PPD, or kernel printer
driver is needed.

## Commands

```text
ipp-printer printers
ipp-printer info auto
ipp-printer info ipp://192.0.2.10:631/ipp/print
ipp-printer print ipp://192.0.2.10:631/ipp/print document.pdf
ipp-printer print auto photo.jpg --media iso_a4_210x297mm --quality high
ipp-printer print auto page.pwg --copies 2 --sides two-sided-long-edge
```

There is no manual discovery step. The app reads the live kernel printer
registry; `auto` waits briefly for the BSP service when the registry is still
warming up. The kernel queries `_ipp._tcp`, the IPP Everywhere subtype, and
`_ipps._tcp` over mDNS. Direct IP URIs remain available on networks that filter
multicast.

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
