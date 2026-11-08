//! Captured launch HOME is authoritative; TRUEOS has no Unix passwd fallback.
pub fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(std::path::PathBuf::from)
}
