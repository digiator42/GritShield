


/// Build a windowed pagination sequence of 1-indexed page numbers with `None` standing
/// in for an ellipsis gap, e.g. current=6, total=20 → [1, …, 4, 5, 6, 7, 8, …, 20].
pub fn build_page_window(current_1based: u64, total: u64) -> Vec<Option<u64>> {
    if total <= 1 {
        return (1..=total).map(Some).collect();
    }

    let lo = current_1based.saturating_sub(2).max(1);
    let hi = (current_1based + 2).min(total);

    let mut pages: Vec<u64> = vec![1];
    for p in lo..=hi {
        if p != 1 {
            pages.push(p);
        }
    }
    if *pages.last().unwrap() != total {
        pages.push(total);
    }
    pages.dedup();

    let mut windowed = Vec::with_capacity(pages.len() + 2);
    for (i, &p) in pages.iter().enumerate() {
        if i > 0 && p > pages[i - 1] + 1 {
            windowed.push(None);
        }
        windowed.push(Some(p));
    }
    windowed
}