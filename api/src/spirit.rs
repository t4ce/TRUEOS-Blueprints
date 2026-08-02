//! Generic Spirit presentation capabilities for Blueprints.
//!
//! These calls do not own an inference session. In particular,
//! [`present_text_silent`] enters only Spirit's bounded visual response queue
//! and never requests local text-to-voice generation.

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Error(pub i32);

fn result(code: i32) -> Result<(), Error> {
    if code == 0 { Ok(()) } else { Err(Error(code)) }
}

/// Queue one of Spirit's model-facing emotion ideas.
pub fn play_emotion(idea: &str) -> Result<(), Error> {
    result(unsafe { v::bp_abi::trueos_cabi_spirit_emotion_play(idea.as_ptr(), idea.len()) })
}

/// Present display-safe UTF-8 without invoking a local inference or voice path.
pub fn present_text_silent(turn: u64, text: &str) -> Result<(), Error> {
    result(unsafe {
        v::bp_abi::trueos_cabi_spirit_text_present_silent(turn, text.as_ptr(), text.len())
    })
}

/// Move Spirit to one normalized scanout point. Both axes are inclusive 0..=1.
pub fn move_to(x_normalized: f32, y_normalized: f32) -> Result<(), Error> {
    if !x_normalized.is_finite()
        || !y_normalized.is_finite()
        || !(0.0..=1.0).contains(&x_normalized)
        || !(0.0..=1.0).contains(&y_normalized)
    {
        return Err(Error(-3));
    }
    result(unsafe { v::bp_abi::trueos_cabi_spirit_move(x_normalized, y_normalized) })
}

#[cfg(test)]
mod tests {
    use super::{Error, move_to};

    #[test]
    fn move_to_rejects_non_normalized_coordinates_before_crossing_the_abi() {
        assert_eq!(move_to(-0.1, 0.5), Err(Error(-3)));
        assert_eq!(move_to(0.5, 1.1), Err(Error(-3)));
        assert_eq!(move_to(f32::NAN, 0.5), Err(Error(-3)));
    }
}
