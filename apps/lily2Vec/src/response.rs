//! Bounded, rate-limited snapshots of an assistant response, using its exact text.
//! This is presentation sampling, not a message parser or an animation state machine.
use crate::{Model, vnni_available};
use anyhow::{Result, anyhow};
use std::{sync::OnceLock, time::Duration};

const SOFT_CHARS: usize = 384;
const RETAIN_CHARS: usize = 768;
const MIN_SENTENCE_CHARS: usize = 96;
const INTERVAL: Duration = Duration::from_millis(2500);
const REPEAT_INTERVAL: Duration = Duration::from_secs(5);

/// Take a recent verbatim section. Prefer paragraph/sentence boundaries before
/// the soft cap, otherwise a word boundary; a long word may reach the hard cap.
pub fn snapshot(text: &str) -> String {
    let mut chars: Vec<_> = text.char_indices().rev().take(RETAIN_CHARS + 1).collect();
    chars.reverse();
    if chars.len() <= SOFT_CHARS {
        return text.to_owned();
    }
    let hard = chars.len().saturating_sub(RETAIN_CHARS);
    let soft = chars.len() - SOFT_CHARS;
    let natural = (hard..=soft).rev().find(|&i| {
        (chars[i].0 == 0)
            || (i > 0
                && (chars[i - 1].1 == '\n'
                    || (chars[i].1.is_whitespace() && matches!(chars[i - 1].1, '.' | '!' | '?'))))
    });
    let word = (hard..=soft)
        .rev()
        .find(|&i| chars[i].0 == 0 || (i > 0 && chars[i - 1].1.is_whitespace()));
    text[chars[natural.or(word).unwrap_or(hard)].0..].to_owned()
}

#[derive(Default)]
pub struct ResponseSampler {
    pending: String,
    last_sample: Option<Duration>,
}
impl ResponseSampler {
    fn due(&self, now: Duration) -> bool {
        self.last_sample
            .is_none_or(|last| now.saturating_sub(last) >= INTERVAL)
    }
    fn append(&mut self, delta: &str) {
        // Bound even a single very large transport delta before concatenation.
        let tail = snapshot(delta);
        if tail.len() != delta.len() {
            self.pending = tail;
            return;
        }
        self.pending.push_str(&tail);
        if self.pending.chars().count() > RETAIN_CHARS {
            self.pending = snapshot(&self.pending);
        }
    }
    pub fn push(&mut self, delta: &str, now: Duration) -> Option<String> {
        self.append(delta);
        let count = self.pending.chars().count();
        let sentence_end = self.pending.trim_end().ends_with(['.', '!', '?']);
        if self.due(now)
            && (count >= SOFT_CHARS
                || self.pending.contains("\n\n")
                || (count >= MIN_SENTENCE_CHARS && sentence_end))
        {
            self.last_sample = Some(now);
            return Some(snapshot(&std::mem::take(&mut self.pending)));
        }
        None
    }
    /// Item completion samples the authoritative tail if cadence permits. A
    /// short complete reply gets a sample even if it never reached the soft cap.
    pub fn finish(&mut self, response: Option<&str>, now: Duration) -> Option<String> {
        let text = response
            .map(snapshot)
            .unwrap_or_else(|| std::mem::take(&mut self.pending));
        self.pending.clear();
        if text.trim().is_empty() || !self.due(now) {
            return None;
        }
        self.last_sample = Some(now);
        Some(snapshot(&text))
    }
    /// Discard unsampled text at interruption, turn change or thread reset.
    pub fn clear(&mut self) {
        self.pending.clear();
    }
}

fn warm_model() -> Result<&'static Model> {
    static MODEL: OnceLock<Result<Model, String>> = OnceLock::new();
    MODEL
        .get_or_init(|| {
            if !vnni_available() {
                return Err("Lily response presentation requires AVX-VNNI + AVX2".into());
            }
            Model::load(crate::DEFAULT_GUIDE).map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|e| anyhow!("{e}"))
}

#[derive(Default)]
pub struct ResponsePresenter {
    sampler: ResponseSampler,
    last_presented: Option<(usize, Duration)>,
}
impl ResponsePresenter {
    pub fn push(
        &mut self,
        delta: &str,
        now: Duration,
        mut submit: impl FnMut(&str) -> bool,
    ) -> Result<()> {
        if let Some(text) = self.sampler.push(delta, now) {
            self.present(&text, now, &mut submit)?;
        }
        Ok(())
    }
    pub fn finish(
        &mut self,
        response: Option<&str>,
        now: Duration,
        mut submit: impl FnMut(&str) -> bool,
    ) -> Result<()> {
        if let Some(text) = self.sampler.finish(response, now) {
            self.present(&text, now, &mut submit)?;
        }
        Ok(())
    }
    pub fn clear(&mut self) {
        self.sampler.clear();
    }
    fn present(
        &mut self,
        text: &str,
        now: Duration,
        submit: &mut impl FnMut(&str) -> bool,
    ) -> Result<()> {
        let model = warm_model()?;
        let query = model.encode(text)?;
        let scores = model.score_vnni(&query)?;
        let index = model.winner(&scores);
        if self
            .last_presented
            .is_some_and(|(last, when)| last == index && now.saturating_sub(when) < REPEAT_INTERVAL)
        {
            return Ok(());
        }
        if submit(&model.labels[index]) {
            self.last_presented = Some((index, now));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }
    #[test]
    fn large_unicode_snapshots_are_bounded_verbatim_suffixes() {
        for text in [
            "🙂愛 ".repeat(10_000),
            "word ".repeat(10_000),
            "x".repeat(10_000),
        ] {
            let sample = snapshot(&text);
            assert!(text.ends_with(&sample));
            assert!(sample.chars().count() <= RETAIN_CHARS);
        }
    }
    #[test]
    fn deltas_are_coalesced_and_completion_obeys_cadence() {
        let mut sampler = ResponseSampler::default();
        assert!(sampler.push("Hello", ms(0)).is_none());
        assert_eq!(
            sampler.push(" there!\n\n", ms(10)).unwrap(),
            "Hello there!\n\n"
        );
        assert!(sampler.push(&"new words ".repeat(200), ms(20)).is_none());
        assert!(sampler.pending.chars().count() <= RETAIN_CHARS);
        assert!(sampler.finish(Some("A short ending"), ms(30)).is_none());
        assert_eq!(
            sampler.finish(Some("A new reply"), ms(3000)).unwrap(),
            "A new reply"
        );
    }
    #[test]
    fn short_reply_and_cancellation_and_authoritative_completion() {
        let mut sampler = ResponseSampler::default();
        sampler.push("discard this", ms(0));
        sampler.clear();
        assert!(sampler.finish(None, ms(100)).is_none());
        sampler.push("incomplete transport", ms(200));
        assert_eq!(
            sampler.finish(Some("complete response"), ms(300)).unwrap(),
            "complete response"
        );
    }
    #[test]
    fn warm_vnni_presenter_drives_labels_without_replay_or_stale_tail() {
        if !vnni_available() {
            return;
        }
        let mut presenter = ResponsePresenter::default();
        let mut cues = Vec::new();
        presenter
            .push("hi", ms(0), |l| {
                cues.push(l.to_owned());
                true
            })
            .unwrap();
        assert!(cues.is_empty());
        presenter
            .finish(Some("hi"), ms(10), |l| {
                cues.push(l.to_owned());
                true
            })
            .unwrap();
        presenter
            .push("i am furious\n\n", ms(20), |_| panic!("cadence violated"))
            .unwrap();
        presenter.clear();
        presenter
            .finish(None, ms(3000), |_| {
                panic!("stale text survived cancellation")
            })
            .unwrap();
        presenter
            .finish(Some("i love you"), ms(4000), |l| {
                cues.push(l.to_owned());
                true
            })
            .unwrap();
        assert_eq!(cues, ["wave", "affection"]);
    }
}
