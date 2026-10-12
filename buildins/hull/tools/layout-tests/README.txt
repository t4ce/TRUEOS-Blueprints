Host layout regressions run the production CSS collector and layout bridge.
Only the kernel download and logging interfaces are mocked.

Run outside the Blueprint Cargo configuration:
  cd /tmp
  cargo test --manifest-path /home/t4ce/Repos/TRUEOS-Blueprints/buildins/hull/tools/layout-tests/Cargo.toml
