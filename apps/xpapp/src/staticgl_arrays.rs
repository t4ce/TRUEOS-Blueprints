// Per-context capacity only. Guest bytes and remap entries are refreshed every draw.
#[derive(Default)]
struct GlDrawScratch {
    raw_indices: Vec<u8>,
    guest_indices: Vec<u32>,
    triangles: Vec<u32>,
    vertices: Vec<crate::staticgl_raster::GlRasterVertex>,
    indices: Vec<u32>,
    dense_remap: Vec<u32>,
    snapshot_ranges: Vec<(u32, Vec<u8>)>,
}

// Draw-local snapshots only: guest arrays may change on the very next draw.
// Coalesce interleaved attributes before crossing the x86 address-space ABI.
struct GlArraySnapshot<'a, M> {
    memory: &'a M,
    ranges: Vec<(u32, Vec<u8>)>,
}

impl<'a, M: GuestMemory> GlArraySnapshot<'a, M> {
    fn new(memory: &'a M, c: &WglContext, indices: &[u32]) -> Self {
        Self::reusing(memory, c, indices, Vec::new())
    }
    fn reusing(memory: &'a M, c: &WglContext, indices: &[u32], ranges: Vec<(u32, Vec<u8>)>) -> Self {
        let mut snapshot = Self { memory, ranges };
        // Clear validity even when this draw cannot be snapshotted.
        for (_, bytes) in &mut snapshot.ranges { bytes.clear(); }
        if cfg!(feature = "replay-arrays") || indices.len() < 3 {
            return snapshot;
        }
        let Some(&lo) = indices.iter().min() else {
            return snapshot;
        };
        let hi = *indices.iter().max().unwrap();
        // Sparse indices must not turn a few vertices into a huge speculative
        // read, or depend on mapping the unused space between those vertices.
        if u64::from(hi) - u64::from(lo) > indices.len() as u64 * 4 {
            return snapshot;
        }
        let needs_normal =
            c.fixed.is_enabled(0xb50) || (0xc60..=0xc63).any(|cap| c.fixed.is_enabled(cap));
        let arrays = [
            c.vertex_pointer.filter(|_| c.vertex_array_enabled),
            c.color_pointer.filter(|_| c.color_array_enabled),
            c.fixed
                .normal_pointer
                .filter(|_| needs_normal && c.fixed.normal_array_enabled),
            c.textures
                .coord_pointer
                .filter(|_| c.textures.enabled && c.textures.coord_array_enabled),
        ];
        let mut spans = [(0u64, 0u64); 4];
        let mut span_count = 0;
        for pointer in arrays.into_iter().flatten() {
            let item = match pointer.kind {
                GL_FLOAT => 4,
                GL_UNSIGNED_BYTE => 1,
                _ => continue,
            };
            let Some(bytes) = pointer
                .size
                .checked_mul(item)
                .filter(|n| *n > 0 && *n <= 16)
            else {
                continue;
            };
            let stride = if pointer.stride == 0 {
                bytes
            } else {
                pointer.stride
            };
            let start = u64::from(pointer.address) + u64::from(lo) * u64::from(stride);
            let end =
                u64::from(pointer.address) + u64::from(hi) * u64::from(stride) + u64::from(bytes);
            if end <= (u32::MAX as u64 + 1) && end - start <= 16 * 1024 * 1024 {
                spans[span_count] = (start, end);
                span_count += 1;
            }
        }
        let spans = &mut spans[..span_count];
        spans.sort_unstable();
        let mut merged = [(0u64, 0u64); 4];
        let mut merged_count = 0;
        for &(start, end) in spans.iter() {
            if let Some(last) = merged[..merged_count].last_mut() {
                if start <= last.1 && end.max(last.1) - last.0 <= 16 * 1024 * 1024 {
                    last.1 = end.max(last.1);
                    continue;
                }
            }
            merged[merged_count] = (start, end);
            merged_count += 1;
        }
        while snapshot.ranges.len() < merged_count { snapshot.ranges.push((0, Vec::new())); }
        for (slot, &(start, end)) in snapshot.ranges.iter_mut().zip(&merged[..merged_count]) {
            slot.0 = start as u32;
            slot.1.resize((end - start) as usize, 0);
            // Failed bulk reads must never expose partially copied or stale bytes.
            if memory.read(slot.0, &mut slot.1).is_err() { slot.1.clear(); }
        }
        snapshot
    }
}

impl<M: GuestMemory> GuestMemory for GlArraySnapshot<'_, M> {
    fn read(&self, address: u32, output: &mut [u8]) -> Result<(), &'static str> {
        for (start, bytes) in &self.ranges {
            if let Some(offset) = address.checked_sub(*start) {
                let offset = offset as usize;
                if let Some(slice) = offset
                    .checked_add(output.len())
                    .and_then(|end| bytes.get(offset..end))
                {
                    output.copy_from_slice(slice);
                    return Ok(());
                }
            }
        }
        self.memory.read(address, output)
    }
    fn write(&mut self, _address: u32, _input: &[u8]) -> Result<(), &'static str> {
        Err("draw snapshot is read-only")
    }
}

enum GlIndexRemap {
    Dense { first: u32, slots: Vec<u32> },
    Sparse(HashMap<u32, u32>),
}
impl GlIndexRemap {
    fn new(indices: &[u32]) -> Self {
        Self::reusing(indices, Vec::new())
    }
    fn reusing(indices: &[u32], mut slots: Vec<u32>) -> Self {
        let first = indices.iter().copied().min().unwrap_or(0);
        let last = indices.iter().copied().max().unwrap_or(0);
        let span = u64::from(last) - u64::from(first) + 1;
        if span <= 1_000_000 && span <= indices.len() as u64 * 4 {
            slots.resize(span as usize, u32::MAX);
            slots.fill(u32::MAX);
            Self::Dense { first, slots }
        } else {
            Self::Sparse(HashMap::new())
        }
    }
    fn get(&self, index: &u32) -> Option<u32> {
        match self {
            Self::Dense { first, slots } => {
                let mapped = slots[(*index - *first) as usize];
                (mapped != u32::MAX).then_some(mapped)
            }
            Self::Sparse(map) => map.get(index).copied(),
        }
    }
    fn insert(&mut self, index: u32, value: u32) {
        match self {
            Self::Dense { first, slots } => slots[(index - *first) as usize] = value,
            Self::Sparse(map) => {
                map.insert(index, value);
            }
        }
    }
}
