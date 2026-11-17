//! Persisted sampling-temperature policy for Lily's expression labels.
//!
//! This selects Pocket TTS sampling variation; it does not add an emotion model
//! or the external pitch, tempo and gain treatments used by the host auditions.
//! Labels use their exact Lily spelling. Other, missing and unknown labels use
//! the default. The current label producer is lily2Vec.

use std::collections::BTreeMap;

pub const RULES_JSON: &str = include_str!("../tone-rules.json");

#[derive(Debug, Clone)]
pub struct ToneRules {
    default_temperature: f32,
    temperature_overrides: BTreeMap<String, f32>,
}

impl ToneRules {
    /// Load once at startup and retain alongside the speech model.
    pub fn embedded() -> Result<Self, String> {
        Self::from_json(RULES_JSON)
    }

    pub fn from_json(json: &str) -> Result<Self, String> {
        let value: serde_json::Value = serde_json::from_str(json)
            .map_err(|error| format!("invalid tone rules JSON: {error}"))?;
        if value.get("schema_version").and_then(|v| v.as_u64()) != Some(1) {
            return Err("unsupported tone rules schema_version; expected 1".into());
        }
        let default_temperature = temperature(
            value
                .get("default_temperature")
                .ok_or("missing default_temperature")?,
            "default_temperature",
        )?;
        let overrides = value
            .get("temperature_overrides")
            .and_then(|v| v.as_object())
            .ok_or("temperature_overrides must be an object")?;
        let mut temperature_overrides = BTreeMap::new();
        for (label, value) in overrides {
            if label.is_empty() || label.chars().any(char::is_whitespace) {
                return Err(format!("invalid Lily label in tone rules: {label:?}"));
            }
            temperature_overrides.insert(label.clone(), temperature(value, label)?);
        }
        Ok(Self {
            default_temperature,
            temperature_overrides,
        })
    }

    pub fn temperature_for(&self, label: Option<&str>) -> f32 {
        label
            .and_then(|label| self.temperature_overrides.get(label))
            .copied()
            .unwrap_or(self.default_temperature)
    }
}

fn temperature(value: &serde_json::Value, field: &str) -> Result<f32, String> {
    let number = value
        .as_f64()
        .ok_or_else(|| format!("tone temperature for {field:?} must be a number"))?
        as f32;
    if !number.is_finite() || number < 0.0 {
        return Err(format!(
            "tone temperature for {field:?} must be finite and nonnegative"
        ));
    }
    Ok(number)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_rules_match_the_selected_temperatures() {
        let rules = ToneRules::embedded().unwrap();
        for label in ["Tired", "Cry"] {
            assert_eq!(rules.temperature_for(Some(label)), 0.2, "{label}");
        }
        for label in ["Happy", "Angry", "Idea"] {
            assert_eq!(rules.temperature_for(Some(label)), 0.4, "{label}");
        }
        assert_eq!(rules.temperature_for(None), 0.3);
    }

    #[test]
    fn other_lily_labels_and_unknown_labels_use_the_default() {
        let rules = ToneRules::embedded().unwrap();
        for label in [
            "Kiss",
            "Blush",
            "Confused",
            "Disapprove",
            "Eat",
            "Facepalm",
            "Gun",
            "FishFace",
            "Flirt",
            "Nod",
            "Scared",
            "Silly",
            "Static",
            "Taunt",
            "Threaten",
            "Waving",
            "Wink",
            "Unknown",
            "",
            "tired",
            "happy",
        ] {
            assert_eq!(rules.temperature_for(Some(label)), 0.3, "{label:?}");
        }
    }

    #[test]
    fn invalid_rules_fail_instead_of_selecting_an_invalid_temperature() {
        assert!(ToneRules::from_json("{}").is_err());
        for invalid in ["-0.1", "\"warm\"", "null", "1e100"] {
            let json = format!(
                r#"{{"schema_version":1,"default_temperature":0.3,"temperature_overrides":{{"Happy":{invalid}}}}}"#
            );
            assert!(ToneRules::from_json(&json).is_err(), "{invalid}");
        }
    }
}
