pub const COLUMNS: usize = 3;
pub fn list_rows(height: u16) -> usize { (height as usize).saturating_sub(7).max(1) }
pub fn page_size(height: u16) -> usize { list_rows(height) * COLUMNS }
pub fn cell(width: u16, position: usize) -> (u16, u16, usize) {
    let column = position % COLUMNS;
    let start = column * width as usize / COLUMNS;
    let end = (column + 1) * width as usize / COLUMNS;
    (start as u16, (position / COLUMNS + 5) as u16, end - start)
}
pub fn hit_test(width: u16, height: u16, selected: usize, count: usize, column: u16, row: u16) -> Option<usize> {
    let offset = row.checked_sub(5)? as usize;
    if column >= width || row >= height.saturating_sub(2) || offset >= list_rows(height) { return None; }
    let column = (0..COLUMNS).find(|&c| (column as usize) < (c + 1) * width as usize / COLUMNS)?;
    let page = page_size(height);
    let position = selected / page * page + offset * COLUMNS + column;
    (position < count).then_some(position)
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn hover_and_click_match_all_three_drawn_columns() {
        for width in [80,81,82,180] {
            for position in 0..51 {
                let (x,y,w) = cell(width,position);
                assert!(w > 0);
                assert_eq!(hit_test(width,24,0,51,x,y),Some(position));
                assert_eq!(hit_test(width,24,0,51,x+w as u16-1,y),Some(position));
            }
        }
    }
    #[test] fn pages_chrome_and_empty_cells_are_bounded() {
        assert_eq!(hit_test(80,24,55,60,0,5),Some(51));
        assert_eq!(hit_test(80,24,55,60,79,8),None);
        for (x,y) in [(0,4),(0,22),(80,5)] { assert_eq!(hit_test(80,24,0,60,x,y),None); }
        assert_eq!(hit_test(80,6,0,60,0,5),None);
    }
}
