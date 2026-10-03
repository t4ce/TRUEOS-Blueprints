//! Host cursor for the single-producer guest light queue.

#[derive(Default)]
pub struct Cursor {
    consumed: usize,
}

impl Cursor {
    pub fn pending(
        &self,
        count: usize,
        capacity: usize,
    ) -> Result<std::ops::Range<usize>, &'static str> {
        if count > capacity {
            return Err("light batch exceeds capacity");
        }
        if count < self.consumed {
            return Err("light batch count moved behind consumed watermark");
        }
        Ok(self.consumed..count)
    }

    pub fn commit(
        &mut self,
        count: usize,
        full_trap: bool,
        capacity: usize,
    ) -> Result<(), &'static str> {
        self.pending(count, capacity)?;
        if full_trap && count != capacity {
            return Err("light batch full trap before capacity");
        }
        self.consumed = if full_trap { 0 } else { count };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Cursor;

    #[test]
    fn interrupted_append_stays_unpublished_until_guest_completes_it() {
        let mut cursor = Cursor::default();
        assert_eq!(cursor.pending(1, 32).unwrap(), 0..1);
        cursor.commit(1, false, 32).unwrap();
        // A timer exits after writing part of slot 1, before count is published.
        assert_eq!(cursor.pending(1, 32).unwrap(), 1..1);
        cursor.commit(1, false, 32).unwrap();
        // The same thread resumes and publishes slot 1.
        assert_eq!(cursor.pending(2, 32).unwrap(), 1..2);
        cursor.commit(2, false, 32).unwrap();
        // A second thread can append after the first thread's completed slot.
        assert_eq!(cursor.pending(3, 32).unwrap(), 2..3);
        cursor.commit(3, false, 32).unwrap();
        assert_eq!(cursor.pending(32, 32).unwrap(), 3..32);
        cursor.commit(32, true, 32).unwrap();
        assert_eq!(cursor.pending(0, 32).unwrap(), 0..0);
        assert_eq!(cursor.pending(1, 32).unwrap(), 0..1);
    }

    #[test]
    fn malformed_count_and_early_full_trap_are_rejected() {
        let mut cursor = Cursor::default();
        assert!(cursor.pending(33, 32).is_err());
        assert!(cursor.commit(1, true, 32).is_err());
        cursor.commit(4, false, 32).unwrap();
        assert!(cursor.pending(3, 32).is_err());
    }
}
