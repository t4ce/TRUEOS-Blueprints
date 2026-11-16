//! Immutable model bytes shipped inside ttstt.bp. English and Heart only.
include!(concat!(env!("OUT_DIR"), "/bundle.rs"));

pub const MODEL_BYTES: usize = 124_081_360;
pub const HEART_BYTES: usize = 522_368;
pub const G2P_BYTES: usize = 6_691_149;
pub const TOTAL_BYTES: usize = MODEL_BYTES + HEART_BYTES + G2P_BYTES;

pub const LICENSES: &str = concat!(
    include_str!("../tools/BUNDLE_SOURCES.md"),
    "\n\n",
    include_str!("../tools/licenses/APACHE-2.0.txt"),
    "\n\n",
    include_str!("../tools/licenses/G2P2-MIT.txt"),
    "\n\n",
    include_str!("../tools/licenses/TAYLORCHU-KOKORO-ONNX-LICENSE.txt"),
    "\n\n",
    include_str!("../tools/licenses/G2P2-GERMANIC-METADATA.json"),
    "\n\n",
    include_str!("../tools/licenses/KOKORO-82M-MODEL-CARD.md"),
    "\n\nUnicode emoji data:\n",
    include_str!("../crates/trueos-kokoro/g2p/LICENSE-UNICODE"),
);

pub fn model() -> &'static [u8] {
    &MODEL.0
}
pub fn heart() -> &'static [u8] {
    &HEART.0
}
pub fn g2p() -> &'static [u8] {
    &G2P.0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundle_has_only_required_english_heart_assets_with_native_alignment() {
        assert_eq!(model().len(), MODEL_BYTES);
        assert_eq!(heart().len(), HEART_BYTES);
        assert_eq!(g2p().len(), G2P_BYTES);
        for bytes in [model(), heart(), g2p()] {
            assert_eq!(bytes.as_ptr() as usize % 64, 0);
        }
        assert_eq!(TOTAL_BYTES, 131_294_877);
        trueos_kokoro_g2p::Model::parse_pinned_english(g2p()).unwrap();
        trueos_kokoro_voice::Voice::parse_heart_npy(heart()).unwrap();
    }
}
