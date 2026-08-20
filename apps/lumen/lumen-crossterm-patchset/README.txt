TRUEOS / TRUEOS-Blueprints — Lumen Crossterm terminal patchset

Read bases used:
  TRUEOS true:            24b503c12f35e885ec76d3d0e62b6310bd0ed3c8
  TRUEOS-Blueprints main: b98512a8285e15d46a1be46f41c7b40f33b2808a

The Blueprint patch touches only:
  apps/lumen/Cargo.toml
  apps/lumen/src/main.rs

It is independent of the Mio RegistrationMode patch in vendor/mio-1.2.0 and can
be applied after that patch.

Apply TRUEOS:
  cd ~/Repos/TRUEOS
  git apply --check /path/to/0001-TRUEOS-vmx-tui-parked-control.patch
  git apply /path/to/0001-TRUEOS-vmx-tui-parked-control.patch

Apply TRUEOS-Blueprints:
  cd ~/Repos/TRUEOS-Blueprints
  git apply --check /path/to/0002-TRUEOS-Blueprints-lumen-crossterm-terminal.patch
  git apply /path/to/0002-TRUEOS-Blueprints-lumen-crossterm-terminal.patch

Suggested checks:
  cargo fmt --all
  git diff --check
  # then run the normal Blueprint build for lumen

Behavior:
  * Lumen is a normal Crossterm terminal Blueprint, not a --vmx-minishell text app.
  * Typed keys, editing, history, paste and Enter are read through Crossterm.
  * Esc or F10 restores Shell2 without terminating the Lumen/model session.
  * `vmx_tui` requests the existing terminal lease back; Lumen accepts the
    reentry and resumes the same logical/model state.
  * Ctrl-Q, Ctrl-C, `quit`, `:quit`, `.quit`, or `:q` terminates the Blueprint.
  * Replication PreparePause remains serviced while the terminal is parked.
  * Raw model replies are written to the structured application log rather than
    mixed into the terminal byte stream.

The small TRUEOS patch is required because the existing command admission gate
rejects all VMX control lines while a terminal lease is Parked. It now keeps
host vmx_* controls available while ordinary Blueprint passthrough remains gated.
