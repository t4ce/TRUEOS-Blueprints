# Nix Blueprint smoke package

`ferris-says-source.nix` is the first enforced Nix boundary in this stack. It
uses `fetchFromGitHub` to fetch `rust-lang/ferris-says` at an exact commit and
verify the unpacked source tree against a recursive SHA-256 hash.

Fetch only that source through Nix:

```text
nix-build nix -A ferris-says-source \
  -I nixpkgs=/home/t4ce/Repos/nixpkgs
```

`ferris-says-blueprint.nix` remains the first Nix-consumable TRUEOS Blueprint
package. Its passthrough metadata references the same source derivation, so the
Nix-side revision and hash now have one authoritative definition.

Build it against the TRUEOS Nixpkgs fork:

```text
nix-build nix -A ferris-says-blueprint \
  -I nixpkgs=/home/t4ce/Repos/nixpkgs
```

The result contains:

```text
share/trueos/blueprints/ferris-says-nix.bp
```

The Blueprint derivation still packages the checked output of the canonical
`cargo bp` builder; it does not compile the fetched source yet. The packer
disables 7-Zip timestamps, so rebuilding an unchanged module produces the same
`.bp` bytes. Moving the exact Rust toolchain, Cargo dependency closure, source
overlay, and Blueprint packer into Nix derivations is the next step before
Hydra can rebuild the `.bp` from source rather than consume the checked
artifact.
