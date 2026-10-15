use std::alloc::{Layout, alloc, alloc_zeroed, dealloc, realloc as rust_realloc};
use std::ffi::c_void;
use std::time::{Duration, Instant};

const MAX_BLOCKS: usize = 50_000;
const BATCH: usize = 256;
const SIZES: [usize; 8] = [48, 64, 96, 128, 192, 256, 384, 512];
type Result<T> = std::result::Result<T, &'static str>;

unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn calloc(count: usize, size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
    fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void;
    fn malloc_usable_size(ptr: *mut c_void) -> usize;
}

fn record(
    state: &'static str,
    case: &'static str,
    blocks: usize,
    ops: usize,
    elapsed: Duration,
    max_batch: Duration,
) {
    let args = format_args!(
        "alloc_probe: {state} case={case} blocks={blocks} ops={ops} elapsed_us={} max_batch_us={} batch={BATCH}",
        elapsed.as_micros(),
        max_batch.as_micros(),
    );
    #[cfg(target_os = "trueos")]
    trueos::logl::log(trueos::logl::level::INFO, args);
    #[cfg(not(target_os = "trueos"))]
    println!("{args}");
}

fn require(condition: bool, error: &'static str) -> Result<()> {
    if condition { Ok(()) } else { Err(error) }
}

/// Owns exactly one valid Rust allocation. Every error path still releases it.
struct Block {
    ptr: *mut u8,
    layout: Layout,
    witness: u8,
}

impl Block {
    fn new(size: usize, align: usize, witness: u8, zeroed: bool) -> Result<Self> {
        let layout = Layout::from_size_align(size, align).map_err(|_| "layout")?;
        let ptr = unsafe {
            if zeroed {
                alloc_zeroed(layout)
            } else {
                alloc(layout)
            }
        };
        require(!ptr.is_null(), "rust_allocation_null")?;
        let block = Self {
            ptr,
            layout,
            witness,
        };
        require(ptr as usize % align == 0, "rust_alignment")?;
        if zeroed {
            require(
                unsafe { std::slice::from_raw_parts(ptr, size) }
                    .iter()
                    .all(|&b| b == 0),
                "rust_zeroed",
            )?;
        }
        unsafe { std::ptr::write_bytes(ptr, witness, size) };
        Ok(block)
    }

    fn check(&self) -> Result<()> {
        // All requested bytes are checked; usable tail bytes are never read.
        require(
            unsafe { std::slice::from_raw_parts(self.ptr, self.layout.size()) }
                .iter()
                .all(|&b| b == self.witness),
            "rust_data_preservation",
        )
    }

    fn resize(&mut self, size: usize) -> Result<()> {
        self.check()?;
        let old_size = self.layout.size();
        let next_layout =
            Layout::from_size_align(size, self.layout.align()).map_err(|_| "realloc_layout")?;
        let next = unsafe { rust_realloc(self.ptr, self.layout, size) };
        require(!next.is_null(), "rust_realloc_null")?;
        // realloc succeeded: ownership must be updated before further checks.
        self.ptr = next;
        self.layout = next_layout;
        require(
            next as usize % self.layout.align() == 0,
            "rust_realloc_alignment",
        )?;
        require(
            unsafe { std::slice::from_raw_parts(next, old_size.min(size)) }
                .iter()
                .all(|&b| b == self.witness),
            "rust_realloc_preservation",
        )?;
        unsafe { std::ptr::write_bytes(next, self.witness, size) };
        Ok(())
    }
}

impl Drop for Block {
    fn drop(&mut self) {
        unsafe { dealloc(self.ptr, self.layout) };
    }
}

struct CBlock(*mut c_void);
impl Drop for CBlock {
    fn drop(&mut self) {
        unsafe { free(self.0) };
    }
}

struct Arena(Vec<Option<Block>>);

impl std::ops::Deref for Arena {
    type Target = Vec<Option<Block>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for Arena {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        let live = self.0.iter().filter(|slot| slot.is_some()).count();
        if live == 0 {
            return;
        }
        // A cooperative deadline cannot invalidate owned live pointers. Make
        // their mandatory release observable even on a slow old kernel.
        let blocks = self.0.len();
        record(
            "BEGIN",
            "cleanup",
            blocks,
            0,
            Duration::ZERO,
            Duration::ZERO,
        );
        let start = Instant::now();
        let mut batch_start = start;
        let mut last_progress = start;
        let mut max_batch = Duration::ZERO;
        let mut released = 0;
        for slot in &mut self.0 {
            if let Some(block) = slot.take() {
                drop(block);
                released += 1;
                if released % BATCH == 0 || released == live {
                    let now = Instant::now();
                    max_batch = max_batch.max(now.duration_since(batch_start));
                    batch_start = now;
                    if now.duration_since(last_progress) >= Duration::from_secs(1) {
                        record(
                            "PROGRESS",
                            "cleanup",
                            blocks,
                            released,
                            now.duration_since(start),
                            max_batch,
                        );
                        last_progress = now;
                    }
                }
            }
        }
        record(
            "PASS",
            "cleanup",
            blocks,
            released,
            start.elapsed(),
            max_batch,
        );
    }
}

fn c_contract() -> Result<()> {
    unsafe { free(std::ptr::null_mut()) };
    for size in [1, 7, 16, 63, 128, 4096, 65_537] {
        let mut block = CBlock(unsafe { malloc(size) });
        require(!block.0.is_null(), "malloc_null")?;
        require(block.0 as usize % 16 == 0, "malloc_alignment")?;
        require(
            unsafe { malloc_usable_size(block.0) } >= size,
            "malloc_usable_size",
        )?;
        unsafe { std::ptr::write_bytes(block.0, 0xa7, size) };
        for new_size in [size * 2 + 17, (size / 2).max(1)] {
            let old_size = unsafe { malloc_usable_size(block.0) };
            // Only the initialized requested prefix is a content witness.
            let preserved = if new_size > size { size } else { new_size };
            require(old_size >= preserved, "realloc_old_usable_size")?;
            let next = unsafe { realloc(block.0, new_size) };
            require(!next.is_null(), "realloc_null")?;
            block.0 = next;
            require(
                unsafe { malloc_usable_size(next) } >= new_size,
                "realloc_usable_size",
            )?;
            require(
                unsafe { std::slice::from_raw_parts(next.cast::<u8>(), preserved) }
                    .iter()
                    .all(|&b| b == 0xa7),
                "realloc_preservation",
            )?;
            unsafe { std::ptr::write_bytes(next, 0xa7, new_size) };
        }
    }
    let zero = CBlock(unsafe { calloc(257, 17) });
    require(!zero.0.is_null(), "calloc_null")?;
    require(
        unsafe { malloc_usable_size(zero.0) } >= 257 * 17,
        "calloc_usable_size",
    )?;
    require(
        unsafe { std::slice::from_raw_parts(zero.0.cast::<u8>(), 257 * 17) }
            .iter()
            .all(|&b| b == 0),
        "calloc_zeroed",
    )?;
    Ok(())
}

fn rust_contract() -> Result<()> {
    for align in [1, 8, 16, 64, 256, 4096, 65_536] {
        for size in [1, 17, 129, 4097] {
            let mut block = Block::new(size, align, 0x5d, true)?;
            block.resize(size * 3 + 7)?;
            block.resize((size / 2).max(1))?;
            block.check()?;
        }
    }
    Ok(())
}

fn phase(
    name: &'static str,
    blocks: usize,
    count: usize,
    budget: Duration,
    mut op: impl FnMut(usize) -> Result<()>,
) -> Result<()> {
    record("BEGIN", name, blocks, 0, Duration::ZERO, Duration::ZERO);
    let start = Instant::now();
    let mut batch_start = start;
    let mut last_progress = start;
    let mut max_batch = Duration::ZERO;
    for index in 0..count {
        if let Err(error) = op(index) {
            record("FAIL", name, blocks, index, start.elapsed(), max_batch);
            return Err(error);
        }
        if (index + 1) % BATCH == 0 || index + 1 == count {
            let now = Instant::now();
            max_batch = max_batch.max(now.duration_since(batch_start));
            batch_start = now;
            if now.duration_since(last_progress) >= Duration::from_secs(1) {
                record(
                    "PROGRESS",
                    name,
                    blocks,
                    index + 1,
                    now.duration_since(start),
                    max_batch,
                );
                last_progress = now;
            }
            if now.duration_since(start) > budget {
                record(
                    "LIMIT",
                    name,
                    blocks,
                    index + 1,
                    now.duration_since(start),
                    max_batch,
                );
                return Err("phase_budget");
            }
        }
    }
    record("PASS", name, blocks, count, start.elapsed(), max_batch);
    Ok(())
}

fn shuffle(values: &mut [usize]) {
    let mut state = 0x6a09_e667_f3bc_c909u64;
    for index in (1..values.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        values.swap(index, state as usize % (index + 1));
    }
}

fn stress(blocks: usize, budget: Duration) -> Result<()> {
    // All bookkeeping reserves happen before the measured allocation shape.
    let mut slots = Arena(Vec::with_capacity(blocks));
    let mut random_order: Vec<usize> = (1..blocks).step_by(2).collect();
    shuffle(&mut random_order);
    phase("allocate", blocks, blocks, budget, |index| {
        slots.push(Some(Block::new(
            SIZES[index % SIZES.len()],
            16,
            (index % 251) as u8,
            false,
        )?));
        Ok(())
    })?;
    // Sort without allocation so alternating frees are truly by address.
    slots.sort_unstable_by_key(|slot| slot.as_ref().unwrap().ptr as usize);
    phase(
        "alternating_free",
        blocks,
        blocks.div_ceil(2),
        budget,
        |index| {
            let block = slots[index * 2].take().unwrap();
            block.check()?;
            drop(block);
            Ok(())
        },
    )?;
    // 256 targets in each address quartile retain nearly the whole fragmented
    // shape. Allocation precedes each free, so the target stays live throughout
    // replacement allocation and is freed exactly once.
    for (quantile, name) in ["churn_q0", "churn_q1", "churn_q2", "churn_q3"]
        .into_iter()
        .enumerate()
    {
        let begin = blocks * quantile / 4;
        let end = blocks * (quantile + 1) / 4;
        let odd_begin = begin | 1;
        let candidates = (end.saturating_sub(odd_begin) + 1) / 2;
        let count = candidates.min(256);
        phase(name, blocks, count, budget, |index| {
            let slot_index = odd_begin + 2 * (index * candidates / count);
            let old = slots[slot_index].as_ref().unwrap();
            old.check()?;
            let next = Block::new(old.layout.size(), old.layout.align(), old.witness, false)?;
            drop(slots[slot_index].replace(next));
            Ok(())
        })?;
    }
    phase(
        "fragmented_realloc",
        blocks,
        256.min(random_order.len()),
        budget,
        |index| {
            let block = slots[random_order[index]].as_mut().unwrap();
            block.resize(block.layout.size() + 31)
        },
    )?;
    phase("random_free", blocks, random_order.len(), budget, |index| {
        let block = slots[random_order[index]].take().unwrap();
        block.check()?;
        drop(block);
        Ok(())
    })?;
    require(
        slots.iter().all(Option::is_none),
        "live_blocks_after_cleanup",
    )
}

fn run() -> Result<()> {
    let mut selected = None;
    let mut budget = Duration::from_secs(15);
    for arg in std::env::args().skip(1) {
        if let Some(value) = arg.strip_prefix("--blocks=") {
            let count: usize = value.parse().map_err(|_| "argument_blocks")?;
            require((16..=MAX_BLOCKS).contains(&count), "argument_blocks")?;
            selected = Some(count);
        } else if let Some(value) = arg.strip_prefix("--phase-ms=") {
            let millis: u64 = value.parse().map_err(|_| "argument_phase_ms")?;
            require((1..=120_000).contains(&millis), "argument_phase_ms")?;
            budget = Duration::from_millis(millis);
        } else {
            return Err("unknown_argument");
        }
    }
    phase("c_contract", 0, 1, budget, |_| c_contract())?;
    phase("rust_contract", 0, 1, budget, |_| rust_contract())?;
    let defaults = [3_000, 10_000, MAX_BLOCKS];
    let one;
    let levels: &[usize] = if let Some(count) = selected {
        one = [count];
        &one
    } else {
        &defaults
    };
    for &blocks in levels {
        let start = Instant::now();
        stress(blocks, budget)?;
        record(
            "PASS",
            "level",
            blocks,
            blocks,
            start.elapsed(),
            Duration::ZERO,
        );
        // Preserve the small-level diagnosis without escalating a slow old
        // kernel into the 50k case. Explicit --blocks bypasses this admission.
        if selected.is_none() && blocks != MAX_BLOCKS && start.elapsed() > Duration::from_secs(2) {
            record(
                "LIMIT",
                "next_level",
                blocks,
                0,
                start.elapsed(),
                Duration::ZERO,
            );
            return Err("level_admission_budget");
        }
    }
    Ok(())
}

fn main() {
    let start = Instant::now();
    record("BEGIN", "all", 0, 0, Duration::ZERO, Duration::ZERO);
    match run() {
        Ok(()) => record("PASS", "all", 0, 0, start.elapsed(), Duration::ZERO),
        Err(error) => {
            // Error identifiers are fixed source literals; no pointers,
            // environment, paths, or witness contents are emitted.
            record(
                if error.ends_with("budget") {
                    "LIMIT"
                } else {
                    "FAIL"
                },
                error,
                0,
                0,
                start.elapsed(),
                Duration::ZERO,
            );
            std::process::exit(1);
        }
    }
}
