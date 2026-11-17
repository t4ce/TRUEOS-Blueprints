//! Authenticated, immutable English model and Azelma voice; no runtime downloads.
include!(concat!(env!("OUT_DIR"), "/bundle.rs"));

pub fn model() -> &'static [u8] {
    &MODEL.0
}
pub fn voice() -> &'static [u8] {
    &AZELMA.0
}
pub fn config() -> &'static [u8] {
    include_bytes!("../model-config.json")
}

pub fn tokenizer() -> &'static [u8] {
    &TOKENIZER.0
}
