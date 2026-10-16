# SQLx remote SQLite probe

This probe exercises the production remote SQLx adapter and SQLite service. Its
contract suite checks typed values, nested and dropped transactions, disconnect
rollback, 16 concurrent updates, six Codex migration sets, read-only existence
checks, errors and persistence across close/reopen.

Build and publish to the existing probe Apps destination:

```sh
probes/sqlx_client/build_publish.sh
```

Launch `§§sqlx_client` on the owned native test instance. The adapter checks its
ordinary SQLite endpoint and automatically requests service startup if needed.
The probe retains ordinary app filesystem confinement. Results are written to
`/common/sqlx-client-report.json`.

The latest isolated native attempt failed during DNS resolution before the
Appstore catalog connection and before SQLite launched. That attempt does not
validate or reject the SQLite contract changes. Host tests exercise the actual
service and adapter; successful native service startup is still required to run
this suite on TRUEOS.
