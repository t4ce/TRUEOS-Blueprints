//! Separate no-menu terrain experiment using the preserved Voxy headless path.
fn main() {
    if let Err(error) = veloren_voxygen::headless::run() {
        eprintln!("Voxy terrain bring-up: {error}");
    }
}
