# IDEdit HTML development server

`idedit_htmlbuild_srv` is an on-demand Blueprint used by IDEdit's HTML project
mode. It is distributed through Apps, alongside other development tools.
Edit and Hull are buildins; this IDEdit subsystem is not.

In Edit, choose **IDE → New Project → Create HTML**, then **Preview**. Edit
saves the project and starts its server child, fetching the Blueprint through
Apps if missing. Hull opens the local preview URL. Closing or switching the
Edit project stops its server; Hull keeps its normal independent lifetime.

The server reads saved project revisions through the existing IDE source
service, whose shared redb image lives in RAM. It replaces its in-memory source
snapshot when the revision changes. There is no exported workspace directory,
folder watcher, or second source store. Unsaved typing is not published.

Hull checks the revision of marked local preview pages every 750 ms and reloads
when it changes. The source service is checked every 500 ms, so Save normally
appears within about 1.25 seconds plus page load time. **F5** requests a refresh
on demand. Regular pages do not enable this development refresh path.
Each asset response uses one immutable revision; requests overlapping a Save
can still span revisions. This is a small development tool, not a general app
server or a production deployment path.

Startup uses literal workspace directives:

```text
fs-scope trueosfs
ide-endpoint 7
ide-db /apps/common/ide/projects.redb
ide-project My page
ide-entry index.html
```

The server binds an available loopback port and emits an `idedit-htmlbuild/v1`
readiness record on its child output. It serves GET/HEAD from project sources
only, with MIME types and caching disabled; it does not expose database paths.

From `TRUEOS-Blueprints`, build with `cargo bp idedit_htmlbuild_srv`. Host checks
can run from outside the kernel checkout with:

```sh
cargo test --manifest-path /path/to/TRUEOS-Blueprints/apps/idedit_htmlbuild_srv/Cargo.toml
cargo test --manifest-path /path/to/TRUEOS-Blueprints/crates/trueos-ide/Cargo.toml
cargo test --manifest-path /path/to/TRUEOS-Buildins/edit/Cargo.toml -p edit --bin edit --features ide
cargo test --manifest-path /path/to/TRUEOS-Buildins/hull/tools/js-tests/Cargo.toml
```

On a host, the server accepts a startup-script filename and reads the same redb
RAM-image schema from `ide-db` for process-level smoke tests. TRUEOS uses the
existing source service directly and never reopens the shared database file.
