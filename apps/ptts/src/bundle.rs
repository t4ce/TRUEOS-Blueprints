//! Embedded voice, tokenizer and config, plus the pin for the external model.
include!(concat!(env!("OUT_DIR"), "/bundle.rs"));

pub fn voice() -> &'static [u8] {
    &AZELMA.0
}
pub fn config() -> &'static [u8] {
    include_bytes!("../model-config.json")
}

pub fn tokenizer() -> &'static [u8] {
    &TOKENIZER.0
}
