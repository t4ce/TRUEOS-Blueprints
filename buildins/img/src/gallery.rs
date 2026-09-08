//! Pure folder ordering/navigation rules, shared by terminal and UI4 input.
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone)]
pub struct Gallery {
    pub paths: Vec<String>,
    pub index: usize,
}

pub fn next_index(index: usize, count: usize, forward: bool) -> Option<usize> {
    if count == 0 || index >= count {
        return None;
    }
    Some(if forward {
        if index + 1 == count { 0 } else { index + 1 }
    } else if index == 0 {
        count - 1
    } else {
        index - 1
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_wraps_both_ends_and_handles_single_or_empty_folders() {
        assert_eq!(next_index(0, 3, false), Some(2));
        assert_eq!(next_index(2, 3, true), Some(0));
        assert_eq!(next_index(1, 3, true), Some(2));
        assert_eq!(next_index(1, 3, false), Some(0));
        assert_eq!(next_index(0, 1, true), Some(0));
        assert_eq!(next_index(0, 1, false), Some(0));
        assert_eq!(next_index(0, 0, true), None);
        assert_eq!(next_index(3, 3, false), None);
    }
}
