// The generated declaration references the one authored kernel helper, even
// when this package is copied into the Blueprint SDK's private staging tree.
include!(concat!(env!("OUT_DIR"), "/vnni_module.rs"));

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

#[derive(Deserialize)]
struct Guide {
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

fn normalize(text: &str) -> String {
    let mut out = String::from(" ");
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if c != '\'' && c != '’' && !out.ends_with(' ') {
            out.push(' ');
        }
    }
    if !out.ends_with(' ') {
        out.push(' ');
    }
    out
}

impl Model {
    pub fn load(json: &str) -> Result<Self> {
        let guide: Guide = serde_json::from_str(json)?;
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
        // First label wins ties. Neutral's small bias handles unknown words.
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

#[cfg(test)]
mod tests {
    use super::*;
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
            ("hi", "wave"),
            ("i don't feel well", "sad"),
            ("i love you", "affection"),
            ("cute", "affection"),
            ("sweet", "happy"),
            ("NO", "disapprove"),
            ("yes that will do", "agree"),
            ("not angry, just explaining", "neutral"),
            ("what if we try it", "idea"),
            ("no clue", "confused"),
            ("unrecognizablexyz", "neutral"),
            ("this", "neutral"),
        ] {
            let q = model.encode(text).unwrap();
            let reference = model.score_reference(&q);
            assert_eq!(model.labels[model.winner(&reference)], expected, "{text}");
            if vnni_available() {
                assert_eq!(model.score_vnni(&q).unwrap(), reference);
            }
        }
    }
}
