These certificates and leaf private keys are synthetic fixture material only.
They are unrelated to accounts, deployment credentials or production TLS.

`ca.pem` trusts `cert.pem`; `wrong-cert.pem` was signed by a separate test CA
whose public certificate is deliberately absent from the probe trust bundle.
Both leaf certificates have IP SANs `127.0.0.1`, `192.168.178.111` and DNS SANs
`localhost`, `mcp-probe.test`. Their validity is ten years from generation on
2026-10-15. Temporary CA signing keys were discarded after generation.

The probe embeds only the public `ca.pem`, writes it to its temporary directory,
and sets `CODEX_CA_CERTIFICATE` before building Tokio or any HTTP client. The
unmodified Codex route-aware transport loads that normal setting. Certificate
validation remains enabled; the separate wrong-root listener must fail TLS.
