// The generated declaration references the one authored kernel helper, even
// when this package is copied into the Blueprint SDK's private staging tree.
include!(concat!(env!("OUT_DIR"), "/vnni_module.rs"));

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

#[derive(Deserialize)]
struct Guide {
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    bias: HashMap<String, i16>,
    features: Vec<Feature>,
}
#[derive(Deserialize)]
struct Feature {
    text: String,
    weights: HashMap<String, i8>,
}

pub struct Model {
    pub labels: Vec<String>,
    pub weights: Vec<Vec<i8>>,
    pub biases: Vec<i32>,
    phrases: Vec<String>,
    pub dimensions: usize,
}

// Classifier-only cues into the existing authored phrase model. These are
// deliberately small, explicit associations, not Unicode-name expansion or
// voice/animation instructions. Unlisted symbols contribute no cue.
const EMOJI_CUES: [(&str, &str); 15] = [
    ("happy", "😀😃😄😁😆😊🙂😂🤣🥳🎉"),
    ("cry", "😢😭😞😔💔"),
    ("angry", "😠😡🤬"),
    ("confused", "😕🤔"),
    ("nod", "👍👌✅"),
    ("disapprove", "👎❌"),
    ("idea", "💡"),
    ("tired", "😴🥱"),
    ("waving", "👋"),
    ("wink", "😉"),
    ("kiss", "😘💋"),
    ("flirt", "😍🥰❤💕💖💗💞"),
    ("silly", "😛😜🤪🙃"),
    ("scared", "😨😱"),
    ("facepalm", "🤦"),
];
// Authored prose normalization removes this control character. Only this
// private cue boundary can introduce it into the feature-matching string.
const CUE_SEPARATOR: char = '\u{1f}';

fn normalize(text: &str) -> String {
    let mut out = String::from(" ");
    let mut cues = 0_u16;
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else {
            if c != '\'' && c != '’' && !out.ends_with(' ') {
                out.push(' ');
            }
            for (index, (_, symbols)) in EMOJI_CUES.iter().enumerate() {
                if symbols.contains(c) {
                    cues |= 1_u16 << index;
                    break;
                }
            }
        }
    }
    if !out.ends_with(' ') {
        out.push(' ');
    }
    // Preserve prose adjacency and negation phrases; emoji between words must
    // not splice a cue into an otherwise contiguous authored phrase. Each cue
    // occurs at most once, independent of repetitions, modifiers and selectors.
    // A reserved boundary prevents the last prose word or the previous cue
    // from forming a synthetic multiword feature with the next cue.
    for (index, (cue, _)) in EMOJI_CUES.iter().enumerate() {
        if cues & (1_u16 << index) != 0 {
            out.push(CUE_SEPARATOR);
            out.push(' ');
            out.push_str(cue);
            out.push(' ');
        }
    }
    out
}

pub const CONTEXTUAL_LABELS: [&str; 16] = [
    "Angry",
    "Blush",
    "Confused",
    "Cry",
    "Disapprove",
    "Drink",
    "Eat",
    "Flirt",
    "Wave",
    "Tired",
    "Grimace",
    "Silly",
    "Scared",
    "Nod",
    "Idea",
    "Happy",
];

fn contextual_label(label: &str) -> Option<&str> {
    Some(match label {
        "Kiss" => "Flirt",
        "Gun" => "Idea",
        "Threaten" => "Angry",
        "Waving" => "Wave",
        "Wink" => "Grimace",
        "Static" => "Static",
        label if CONTEXTUAL_LABELS.contains(&label) => label,
        _ => return None,
    })
}

fn contextual_weights(weights: HashMap<String, i8>) -> HashMap<String, i8> {
    let mut folded = HashMap::<String, i8>::new();
    for (label, weight) in weights {
        if let Some(label) = contextual_label(&label) {
            folded
                .entry(label.to_owned())
                .and_modify(|old| *old = (*old).max(weight))
                .or_insert(weight);
        }
    }
    folded
}

impl Model {
    pub fn load(json: &str) -> Result<Self> {
        Self::from_guide(serde_json::from_str(json)?)
    }

    /// Use the Contextual row's vocabulary for turn playback. The original
    /// read-only classifier still serves callers that use the old cue names.
    pub fn contextual() -> Result<Self> {
        let mut guide: Guide = serde_json::from_str(DEFAULT_GUIDE)?;
        guide.labels = CONTEXTUAL_LABELS
            .into_iter()
            .chain(["Static"])
            .map(str::to_owned)
            .collect();
        for feature in &mut guide.features {
            feature.weights = contextual_weights(std::mem::take(&mut feature.weights));
        }
        guide.features.retain(|feature| !feature.weights.is_empty());
        for (text, label) in [
            ("drink", "Drink"),
            ("drinking", "Drink"),
            ("sip", "Drink"),
            ("grimace", "Grimace"),
        ] {
            guide.features.push(Feature {
                text: text.to_owned(),
                weights: HashMap::from([(label.to_owned(), 8)]),
            });
        }
        Self::from_guide(guide)
    }

    fn from_guide(mut guide: Guide) -> Result<Self> {
        if guide.labels.is_empty() {
            guide.labels = DEFAULT_TOKENS.lines().map(str::to_owned).collect();
        }
        if guide.labels.is_empty()
            || guide.labels.len() > 256
            || guide.features.is_empty()
            || guide.features.len() > 4096
        {
            bail!("guide requires 1..256 labels and 1..4096 features");
        }
        let unique: HashSet<_> = guide.labels.iter().collect();
        if unique.len() != guide.labels.len()
            || guide
                .labels
                .iter()
                .any(|s| s.is_empty() || s.chars().any(char::is_whitespace))
        {
            bail!("labels must be unique, nonempty single tokens");
        }
        let dimensions = guide.features.len().div_ceil(32) * 32;
        let mut weights = vec![vec![0; dimensions]; guide.labels.len()];
        let mut biases = vec![0; guide.labels.len()];
        for (label, bias) in guide.bias {
            let index = guide
                .labels
                .iter()
                .position(|l| *l == label)
                .context("unknown bias label")?;
            biases[index] = i32::from(bias);
        }
        let mut phrases = Vec::new();
        for (column, feature) in guide.features.into_iter().enumerate() {
            let phrase = normalize(&feature.text);
            if phrase.trim().is_empty() || phrases.contains(&phrase) {
                bail!("empty or duplicate feature: {}", feature.text);
            }
            phrases.push(phrase);
            for (label, weight) in feature.weights {
                let row = guide
                    .labels
                    .iter()
                    .position(|l| *l == label)
                    .context("unknown feature label")?;
                weights[row][column] = weight;
            }
        }
        Ok(Self {
            labels: guide.labels,
            weights,
            biases,
            phrases,
            dimensions,
        })
    }

    pub fn encode(&self, text: &str) -> Result<Vec<u8>> {
        if text.trim().is_empty() {
            bail!("provide some words");
        }
        if text.len() > 16_384 {
            bail!("input exceeds 16384 bytes");
        }
        let text = normalize(text);
        let mut query = vec![0; self.dimensions];
        for (index, phrase) in self.phrases.iter().enumerate() {
            query[index] = u8::from(text.contains(phrase));
        }
        Ok(query)
    }

    pub fn winner(&self, scores: &[i32]) -> usize {
        // First label wins ties. The default guide biases Static for unknown words.
        let mut best = 0;
        for index in 1..scores.len() {
            if scores[index] > scores[best] {
                best = index;
            }
        }
        best
    }

    pub fn score_reference(&self, query: &[u8]) -> Vec<i32> {
        assert_eq!(query.len(), self.dimensions);
        self.weights
            .iter()
            .zip(&self.biases)
            .map(|(weights, bias)| {
                weights
                    .iter()
                    .zip(query)
                    .map(|(&w, &x)| i32::from(w) * i32::from(x))
                    .sum::<i32>()
                    + bias
            })
            .collect()
    }

    pub fn score_vnni(&self, query: &[u8]) -> Result<Vec<i32>> {
        if !vnni_available() {
            bail!(
                "AVX-VNNI + AVX2 required; this program does not substitute another scoring backend"
            );
        }
        if query.len() != self.dimensions {
            bail!("feature dimension mismatch");
        }
        #[cfg(target_arch = "x86_64")]
        // Runtime detection checks CPU support and OS-enabled vector state.
        return Ok(self
            .weights
            .iter()
            .zip(&self.biases)
            .map(|(w, b)| unsafe { dot_vnni(query, w) } + b)
            .collect());
        #[cfg(not(target_arch = "x86_64"))]
        unreachable!()
    }
}

pub fn vnni_available() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("avxvnni")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// Actual classifier dot product: unsigned byte features × signed byte weights.
/// VEX VPDPBUSD accumulates four products per i32 lane, eight lanes at a time.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,avxvnni")]
#[inline(never)]
unsafe fn dot_vnni(query: &[u8], weights: &[i8]) -> i32 {
    use std::arch::x86_64::*;
    debug_assert_eq!(query.len(), weights.len());
    debug_assert_eq!(query.len() % 32, 0);
    let mut acc = _mm256_setzero_si256();
    for offset in (0..query.len()).step_by(32) {
        let x = unsafe { _mm256_loadu_si256(query.as_ptr().add(offset).cast()) };
        let w = unsafe { _mm256_loadu_si256(weights.as_ptr().add(offset).cast()) };
        acc = unsafe { vnni::dpbusd_256(acc, x, w) };
    }
    let mut lanes = [0_i32; 8];
    unsafe {
        _mm256_storeu_si256(lanes.as_mut_ptr().cast(), acc);
    }
    lanes.into_iter().sum()
}

/// The guided integer model owned by the Lily service and standalone test CLI.
pub const DEFAULT_GUIDE: &str = include_str!("../vibes.json");
/// The editable classifier vocabulary, independent of Spirit's animation mapping.
pub const DEFAULT_TOKENS: &str = include_str!("../tokens.txt");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contextual_vocabulary_matches_the_preview_names() {
        let model = Model::contextual().unwrap();
        assert_eq!(
            model.labels,
            CONTEXTUAL_LABELS.into_iter().chain(["Static"]).collect::<Vec<_>>()
        );
        for label in CONTEXTUAL_LABELS {
            assert_label(&model, label, label);
        }
        assert_label(&model, "hello", "Wave");
        assert_label(&model, "unrecognizablexyz", "Static");
    }

    fn assert_label(model: &Model, text: &str, expected: &str) {
        let query = model.encode(text).unwrap();
        let reference = model.score_reference(&query);
        assert_eq!(model.labels[model.winner(&reference)], expected, "{text}");
        if vnni_available() {
            assert_eq!(model.score_vnni(&query).unwrap(), reference, "{text}");
        }
    }

    #[test]
    fn curated_emoji_use_existing_phrase_features_and_vnni_scores() {
        let model = Model::load(DEFAULT_GUIDE).unwrap();
        let cases = [
            ("😄", "Happy"),
            ("hello 😄", "Happy"),
            ("😭", "Cry"),
            ("😡", "Angry"),
            ("🤔", "Confused"),
            ("👍", "Nod"),
            ("👎", "Disapprove"),
            ("💡", "Idea"),
            ("😴", "Tired"),
            ("👋", "Waving"),
            ("😉", "Wink"),
            ("😘", "Kiss"),
            ("❤️", "Flirt"),
            ("🤪", "Silly"),
            ("😱", "Scared"),
            ("🤦", "Facepalm"),
            ("🦄🛸🧿", "Static"),
        ];
        for (text, expected) in cases {
            assert_label(&model, text, expected);
        }
        // No new classifier columns, weights, labels or scoring backend.
        for (cue, _) in EMOJI_CUES {
            assert!(model.phrases.contains(&normalize(cue)), "missing authored cue {cue}");
        }
    }

    #[test]
    fn emoji_modifiers_and_repetitions_do_not_amplify_binary_cues() {
        let model = Model::load(DEFAULT_GUIDE).unwrap();
        for (text, canonical, expected) in [
            ("👋🏽", "waving", "Waving"),
            ("🤦🏿\u{200d}♀️", "facepalm", "Facepalm"),
            ("❤\u{fe0e}", "flirt", "Flirt"),
            ("❤\u{fe0f}", "flirt", "Flirt"),
            ("😄😃😊😂🥳🎉", "happy", "Happy"),
            ("🏳️\u{200d}🌈", "unrecognizablexyz", "Static"),
        ] {
            assert_eq!(model.encode(text).unwrap(), model.encode(canonical).unwrap(), "{text}");
            assert_label(&model, text, expected);
        }
        let repeated = "😄".repeat(768);
        assert_eq!(normalize(&repeated), " \u{1f} happy ");
        assert_eq!(model.encode(&repeated).unwrap(), model.encode("😄").unwrap());
    }

    #[test]
    fn emoji_cues_preserve_prose_negation_and_the_original_text() {
        let model = Model::load(DEFAULT_GUIDE).unwrap();
        let original = String::from("not 😄 angry, just explaining");
        let saved = original.clone();
        assert_eq!(normalize(&original), " not angry just explaining \u{1f} happy ");
        assert_label(&model, &original, "Static");
        assert_eq!(original, saved);
        assert_eq!(normalize("left😄right"), " left right \u{1f} happy ");
        assert_eq!(normalize("i don't feel well 🦄"), " i dont feel well ");
        assert_label(&model, "i don't feel well 🦄", "Cry");
        assert_label(&model, "unrecognizablexyz 🦄", "Static");
        assert!(model.encode("  ").is_err());
        assert!(model.encode(&"😄".repeat(4097)).is_err());
    }

    #[test]
    fn cue_boundaries_do_not_create_prose_or_cross_cue_phrases() {
        let model = Model::load(DEFAULT_GUIDE).unwrap();
        let original = "😄 The next item is not";
        assert_eq!(normalize(original), " the next item is not \u{1f} happy ");
        assert_label(&model, original, "Happy");
        // Real prose negation still activates the original authored feature.
        assert_eq!(normalize("not happy"), " not happy ");
        assert_label(&model, "not happy", "Cry");

        let phrases = Model::load(r#"{
            "labels":["Happy","Wink","Angry"],
            "features":[
                {"text":"happy","weights":{"Happy":8}},
                {"text":"wink","weights":{"Wink":12}},
                {"text":"happy wink","weights":{"Angry":100}}
            ]
        }"#).unwrap();
        for text in ["😄😉", "happy😉", "😄 😉 😄"] {
            let query = phrases.encode(text).unwrap();
            assert_eq!(&query[..3], &[1, 1, 0], "{text}");
            assert_label(&phrases, text, "Wink");
        }
        assert_eq!(normalize("😄😉"), " \u{1f} happy \u{1f} wink ");
        // Adjacent literal prose keeps its previous multiword matching.
        assert_label(&phrases, "happy wink", "Angry");
        assert_eq!(normalize("happy\u{1f}wink"), " happy wink ");
    }

    #[test]
    fn vnni_matches_integer_reference_with_negative_weights_and_full_byte_range() {
        if !vnni_available() {
            eprintln!("AVX-VNNI unavailable: hardware test skipped");
            return;
        }
        let mut seed = 17_u32;
        for dimensions in [32, 64, 160, 512, 4096] {
            let mut query = vec![0; dimensions];
            let mut weights = vec![0; dimensions];
            for i in 0..dimensions {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                query[i] = (seed >> 24) as u8;
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                weights[i] = (seed >> 24) as i8;
            }
            let expected: i32 = query
                .iter()
                .zip(&weights)
                .map(|(&x, &w)| i32::from(x) * i32::from(w))
                .sum();
            assert_eq!(unsafe { dot_vnni(&query, &weights) }, expected);
        }
    }
    #[test]
    fn guided_labels_and_phrase_corrections() {
        let model = Model::load(include_str!("../vibes.json")).unwrap();
        for (text, expected) in [
            ("hi", "Waving"),
            ("i don't feel well", "Cry"),
            ("i love you", "Flirt"),
            ("cute", "Flirt"),
            ("sweet", "Happy"),
            ("NO", "Disapprove"),
            ("yes that will do", "Nod"),
            ("not angry, just explaining", "Static"),
            ("what if we try it", "Idea"),
            ("no clue", "Confused"),
            ("unrecognizablexyz", "Static"),
            ("this", "Static"),
        ] {
            let q = model.encode(text).unwrap();
            let reference = model.score_reference(&q);
            assert_eq!(model.labels[model.winner(&reference)], expected, "{text}");
            if vnni_available() {
                assert_eq!(model.score_vnni(&q).unwrap(), reference);
            }
        }
    }

    #[test]
    fn revised_vocabulary_and_context_cues_use_the_same_vnni_scores() {
        let model = Model::load(DEFAULT_GUIDE).unwrap();
        assert_eq!(model.labels, DEFAULT_TOKENS.lines().collect::<Vec<_>>());
        let cases = model
            .labels
            .iter()
            .map(|label| (label.as_str(), label.as_str()))
            .chain([
                ("The bug was a typo", "Silly"),
                ("This approach silently drops errors", "Disapprove"),
                ("We can untangle the source code", "Idea"),
                ("The build passed", "Happy"),
                ("I forgot the semicolon", "Facepalm"),
                ("No errors", "Nod"),
                ("There is no risk of data loss", "Static"),
                ("I need a break", "Tired"),
            ]);
        for (text, expected) in cases {
            let query = model.encode(text).unwrap();
            let scores = model.score_reference(&query);
            assert_eq!(model.labels[model.winner(&scores)], expected, "{text}");
            if vnni_available() {
                assert_eq!(model.score_vnni(&query).unwrap(), scores, "{text}");
            }
        }
    }
}
