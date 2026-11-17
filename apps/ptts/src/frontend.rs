//! Runtime English text preparation for the pinned Azelma model.
//!
//! Only the authenticated JSON tokenizer is used: no SentencePiece C++ code,
//! filesystem access, model downloads or tokenizer worker pool is needed.

#[path = "emoji.rs"]
mod emoji;
#[path = "frontend_text.rs"]
mod text;

use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use text::{TextOptions, TextTokenizer};

const TARGET_CHUNK_TOKENS: usize = 50;
const MAX_CHUNK_TOKENS: usize = 512;
const MAX_TEXT_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone)]
pub struct PreparedChunk {
    /// Speech-only text. The caller retains the original response for display/Lily.
    #[cfg(test)]
    pub text: String,
    pub tokens: Vec<u32>,
    pub frames_after_eos: usize,
}

pub struct Frontend {
    tokenizer: tokenizers::Tokenizer,
    options: TextOptions,
    pronunciation: BTreeMap<String, String>,
}

impl Frontend {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        // A single speech sidekick owns this tokenizer. The crate's atomic
        // override prevents both environment-driven and implicit Rayon startup.
        tokenizers::utils::parallelism::set_parallelism(false);
        let tokenizer = tokenizers::Tokenizer::from_bytes(bytes)
            .map_err(|error| anyhow::anyhow!("invalid bundled tokenizer: {error}"))?;
        if tokenizer.get_vocab_size(true) != 4000 {
            bail!("the bundled English tokenizer must have 4000 entries")
        }
        Ok(Self {
            tokenizer,
            options: TextOptions {
                pad_with_spaces_for_short_inputs: false,
                remove_semicolons: false,
                append_terminal_punctuation: true,
                capitalize_first_letter: true,
                replace_characters: [('’', "'".into()), ('‘', "'".into())].into(),
            },
            pronunciation: BTreeMap::from([
                ("TRUEOS".into(), "True O S".into()),
                ("CPU".into(), "C P U".into()),
            ]),
        })
    }

    /// Prepare all chunks before inference so invalid text cannot publish a prefix.
    /// Emoji-only requests are accepted silently, matching the previous sidekick.
    pub fn prepare(&self, input: &str) -> Result<Vec<PreparedChunk>> {
        if input.len() > MAX_TEXT_BYTES {
            bail!("speech text exceeds {MAX_TEXT_BYTES} UTF-8 bytes")
        }
        if input.trim().is_empty() {
            bail!("speech text must not be empty")
        }
        let spoken = emoji::speech_without_emoji(input).context("emoji preparation failed")?;
        if spoken.trim().is_empty() {
            return Ok(Vec::new());
        }
        let spoken = text::apply_pronunciation_overrides(&spoken, &self.pronunciation);
        let chunks = text::split_into_best_sentences(
            self,
            spoken.as_ref(),
            TARGET_CHUNK_TOKENS,
            &self.options,
        )?;
        chunks
            .into_iter()
            .map(|chunk| {
                let (text, eos_guess) = text::prepare_text_prompt(&chunk, &self.options)?;
                let tokens = self.encode(&text)?;
                // The official chunker treats 50 as a target and can retain a
                // longer unpunctuated clause. Honor the engine's hard bound;
                // never truncate a final answer or silently omit its suffix.
                if tokens.is_empty() || tokens.len() > MAX_CHUNK_TOKENS {
                    bail!(
                        "speech clause contains {} tokens; maximum is {MAX_CHUNK_TOKENS}",
                        tokens.len()
                    )
                }
                Ok(PreparedChunk {
                    #[cfg(test)]
                    text,
                    tokens,
                    frames_after_eos: eos_guess + 2,
                })
            })
            .collect()
    }
}

impl TextTokenizer for Frontend {
    fn encode(&self, text: &str) -> Result<Vec<u32>> {
        Ok(self
            .tokenizer
            .encode(text, true)
            .map_err(|error| anyhow::anyhow!("cannot tokenize speech: {error}"))?
            .get_ids()
            .to_vec())
    }

    fn decode(&self, tokens: &[u32]) -> Result<String> {
        self.tokenizer
            .decode(tokens, true)
            .map_err(|error| anyhow::anyhow!("cannot decode speech tokens: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frontend() -> Frontend {
        Frontend::from_bytes(include_bytes!("../models/tokenizer.json")).unwrap()
    }

    #[test]
    fn approved_fixed_sample_keeps_its_prepared_token_ids() {
        let chunks = frontend().prepare(crate::bundle::SAMPLE_TEXT).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, crate::bundle::SAMPLE_TEXT);
        assert_eq!(chunks[0].tokens, crate::bundle::SAMPLE_TOKENS);
        assert_eq!(chunks[0].frames_after_eos, 3);
    }

    #[test]
    fn speech_aliases_handle_case_without_mutating_display_text() {
        let original = "trueos uses the cpu, while CPU2 and my_cpu stay unchanged".to_string();
        let saved = original.clone();
        let chunks = frontend().prepare(&original).unwrap();
        assert_eq!(original, saved);
        assert_eq!(chunks.len(), 1);
        assert_eq!(
            chunks[0].text,
            "True O S uses the C P U, while CPU2 and my_cpu stay unchanged."
        );
        let chunk = frontend().prepare("hello").unwrap().remove(0);
        assert_eq!(chunk.text, "Hello.");
        assert_eq!(chunk.frames_after_eos, 5);
    }

    #[test]
    fn emoji_only_is_silent_and_mixed_emoji_keeps_word_boundaries() {
        let f = frontend();
        assert!(f.prepare("😄 👩🏽‍💻 🇩🇪 1️⃣").unwrap().is_empty());
        assert_eq!(f.prepare("Ready😄now").unwrap()[0].text, "Ready now.");
        assert!(f.prepare(" \n ").is_err());
    }

    #[test]
    fn long_codex_prose_preserves_urls_decimals_titles_and_all_sentences() {
        let sentence = "Visit https://example.com/v1.2?q=3.5 and ask Dr. Smith. ";
        let input = sentence.repeat(12);
        let chunks = frontend().prepare(&input).unwrap();
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.tokens.len() <= MAX_CHUNK_TOKENS);
            assert!(chunk.text.contains("https://example.com/v1.2?q=3.5"));
            assert!(chunk.text.contains("Dr. Smith."));
        }
        let spoken = chunks
            .iter()
            .map(|chunk| chunk.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            spoken.split_whitespace().collect::<Vec<_>>(),
            input.split_whitespace().collect::<Vec<_>>()
        );
    }

    #[test]
    fn oversized_unpunctuated_clause_is_rejected_instead_of_truncated() {
        let input = "antidisestablishmentarianism ".repeat(400);
        let error = frontend().prepare(&input).unwrap_err().to_string();
        assert!(error.contains("maximum is 512"), "{error}");
    }
}
