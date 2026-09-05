// rust-src adaptation belongs to the selected target's pack step. Merely
// building the host packer must never patch the host compiler's sysroot.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
}
