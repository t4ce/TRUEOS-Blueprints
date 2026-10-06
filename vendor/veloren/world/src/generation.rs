// Scheduling boundaries for the server's synchronous world bootstrap.
// This is a worldgen policy, not a replacement thread scheduler.

pub(crate) struct WorkBudget {
    #[cfg(any(target_os = "trueos", feature = "cooperative-worldgen"))]
    last_turn: std::time::Instant,
}

impl WorkBudget {
    pub(crate) fn new() -> Self {
        Self {
            #[cfg(any(target_os = "trueos", feature = "cooperative-worldgen"))]
            last_turn: std::time::Instant::now(),
        }
    }

    /// Call between complete units of generation work, with no shared lock held.
    /// Logging is deliberately independent of these scheduling points.
    #[inline]
    pub(crate) fn checkpoint(&mut self) {
        #[cfg(any(target_os = "trueos", feature = "cooperative-worldgen"))]
        if self.last_turn.elapsed() >= std::time::Duration::from_millis(10) {
            std::thread::yield_now();
            self.last_turn = std::time::Instant::now();
        }
    }
}

/// Finish a mutable site's update before visiting the next site. The caller
/// retains the phase barrier: trade runs only after all updates return.
#[cfg(any(target_os = "trueos", feature = "cooperative-worldgen"))]
pub(crate) fn for_each_site<T>(sites: impl IntoIterator<Item = T>, mut update: impl FnMut(T)) {
    let mut budget = WorkBudget::new();
    for site in sites {
        update(site);
        budget.checkpoint();
    }
}

/// Visit every cell once, completing generation and insertion before advancing.
#[cfg(any(target_os = "trueos", feature = "cooperative-worldgen"))]
pub(crate) fn for_each_cell<T>(size: [u32; 2], mut generate: impl FnMut([u32; 2]) -> T, mut insert: impl FnMut([u32; 2], T)) {
    let mut budget = WorkBudget::new();
    for x in 0..size[0] {
        for y in 0..size[1] {
            let position = [x, y];
            let value = generate(position);
            insert(position, value);
            budget.checkpoint();
        }
    }
}

/// Retain every result (including absent samples) in input order, without a
/// parallel completion barrier during cooperative bootstrap.
#[cfg(any(target_os = "trueos", feature = "cooperative-worldgen"))]
pub(crate) fn collect_ordered<T, U>(items: impl IntoIterator<Item = T>, mut sample: impl FnMut(T) -> U) -> Vec<U> {
    let items = items.into_iter();
    let mut output = Vec::with_capacity(items.size_hint().0);
    collect_ordered_into(items, &mut sample, &mut output);
    output
}

/// Allow the caller to reserve and diagnose storage separately from sampling.
#[cfg(any(target_os = "trueos", feature = "cooperative-worldgen"))]
pub(crate) fn collect_ordered_into<T, U>(items: impl IntoIterator<Item = T>, mut sample: impl FnMut(T) -> U, output: &mut Vec<U>) {
    let mut budget = WorkBudget::new();
    for item in items {
        let value = sample(item);
        output.push(value);
        budget.checkpoint();
    }
}

/// Report completed work independently of the quiet scheduling budget.
pub(crate) struct ScanProgress {
    #[cfg(target_os = "trueos")]
    group: &'static str,
    #[cfg(target_os = "trueos")]
    stage: &'static str,
    #[cfg(target_os = "trueos")]
    total: usize,
    #[cfg(target_os = "trueos")]
    started: std::time::Instant,
    #[cfg(target_os = "trueos")]
    last_report: std::time::Instant,
}

impl ScanProgress {
    pub(crate) fn new(_stage: &'static str, _total: usize) -> Self {
        Self::for_group("map-data", _stage, _total)
    }

    pub(crate) fn for_group(_group: &'static str, _stage: &'static str, _total: usize) -> Self {
        Self {
            #[cfg(target_os = "trueos")]
            group: _group,
            #[cfg(target_os = "trueos")]
            stage: _stage,
            #[cfg(target_os = "trueos")]
            total: _total,
            #[cfg(target_os = "trueos")]
            started: std::time::Instant::now(),
            #[cfg(target_os = "trueos")]
            last_report: std::time::Instant::now(),
        }
    }

    pub(crate) fn completed(&mut self, _completed: usize) {
        #[cfg(target_os = "trueos")]
        if _completed == 1 || _completed == self.total
            || self.last_report.elapsed() >= std::time::Duration::from_secs(2)
        {
            eprintln!("velosrv: {} stage={} completed={}/{} elapsed_ms={}",
                self.group, self.stage, _completed, self.total, self.started.elapsed().as_millis());
            self.last_report = std::time::Instant::now();
        }
    }
}
