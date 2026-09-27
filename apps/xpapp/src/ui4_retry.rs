//! Retry UI4 backpressure at an individual acquisition/publication boundary.
//! Never repeat the drawing work or turn a permanent error into a wait.
use trueos::ui4_scene::Error;

pub fn retry_busy<T>(
    mut operation: impl FnMut() -> Result<T, Error>,
    mut wait: impl FnMut(),
) -> Result<T, Error> {
    loop {
        match operation() {
            Err(Error::Busy) => wait(),
            result => return result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquisition_waits_for_retirement_before_returning_the_lease() {
        let mut calls = 0;
        let mut waits = 0;
        let lease = retry_busy(
            || {
                calls += 1;
                if calls < 4 { Err(Error::Busy) } else { Ok(17) }
            },
            || waits += 1,
        )
        .unwrap();
        assert_eq!((lease, calls, waits), (17, 4, 3));
    }

    #[test]
    fn available_frame_has_no_wait() {
        assert!(retry_busy(|| Ok(()), || panic!("unnecessary wait")).is_ok());
    }

    #[test]
    fn permanent_error_is_not_retried() {
        let mut calls = 0;
        let mut waits = 0;
        let result: Result<(), Error> = retry_busy(
            || {
                calls += 1;
                if calls == 1 {
                    Err(Error::Busy)
                } else {
                    Err(Error::Invalid)
                }
            },
            || waits += 1,
        );
        assert!(matches!(result, Err(Error::Invalid)));
        assert_eq!((calls, waits), (2, 1));
    }
}
