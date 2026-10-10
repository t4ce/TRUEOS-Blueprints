use std::time::{Duration, Instant};

pub fn list_rows(height: u16) -> usize {
    (height as usize).saturating_sub(7).max(1)
}

pub fn hit_test(width: u16, height: u16, selected: usize, count: usize, column: u16, row: u16) -> Option<usize> {
    let rows = list_rows(height);
    let offset = row.checked_sub(5)? as usize;
    if column >= width || row >= height.saturating_sub(2) || offset >= rows {
        return None;
    }
    let position = selected / rows * rows + offset;
    (position < count).then_some(position)
}

#[derive(Default)]
pub struct Clicks(Option<(usize, Instant)>);

impl Clicks {
    pub fn reset(&mut self) { self.0 = None; }

    pub fn click(&mut self, position: usize, now: Instant) -> bool {
        let launch = self.0.is_some_and(|(previous, time)| previous == position && now.duration_since(time) <= Duration::from_millis(500));
        self.0 = if launch { None } else { Some((position, now)) };
        launch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicks_map_to_the_drawn_page_and_ignore_chrome_and_empty_rows() {
        assert_eq!(hit_test(80, 24, 19, 40, 4, 5), Some(17));
        assert_eq!(hit_test(80, 24, 19, 40, 4, 21), Some(33));
        for (column, row) in [(4, 4), (4, 22), (80, 5)] {
            assert_eq!(hit_test(80, 24, 19, 40, column, row), None);
        }
        assert_eq!(hit_test(80, 24, 0, 2, 4, 7), None);
        assert_eq!(hit_test(80, 6, 0, 2, 4, 5), None);
        assert_eq!(hit_test(80, 14, 19, 40, 4, 5), Some(14));
    }

    #[test]
    fn launch_requires_two_recent_clicks_on_the_same_item() {
        let mut clicks = Clicks::default();
        let now = Instant::now();
        assert!(!clicks.click(1, now));
        assert!(!clicks.click(2, now + Duration::from_millis(100)));
        assert!(clicks.click(2, now + Duration::from_millis(200)));
        assert!(!clicks.click(2, now + Duration::from_millis(300)));
        assert!(!clicks.click(2, now + Duration::from_millis(900)));
        clicks.reset();
        assert!(!clicks.click(2, now + Duration::from_millis(950)));
    }
}
