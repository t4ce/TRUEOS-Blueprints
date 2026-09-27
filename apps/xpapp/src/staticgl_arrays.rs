// Draw-local snapshots only: guest arrays may change on the very next draw.
// Coalesce interleaved attributes before crossing the x86 address-space ABI.
struct GlArraySnapshot<'a, M> {
    memory: &'a M,
    ranges: Vec<(u32, Vec<u8>)>,
}

impl<'a, M: GuestMemory> GlArraySnapshot<'a, M> {
    fn new(memory: &'a M, c: &WglContext, indices: &[u32]) -> Self {
        let mut snapshot = Self {
            memory,
            ranges: Vec::new(),
        };
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
        let mut spans = Vec::new();
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
                spans.push((start, end));
            }
        }
        spans.sort_unstable();
        let mut merged: Vec<(u64, u64)> = Vec::new();
        for (start, end) in spans {
            if let Some(last) = merged.last_mut() {
                if start <= last.1 && end.max(last.1) - last.0 <= 16 * 1024 * 1024 {
                    last.1 = end.max(last.1);
                    continue;
                }
            }
            merged.push((start, end));
        }
        for (start, end) in merged {
            let mut bytes = vec![0; (end - start) as usize];
            // An unmapped unused gap is not a GL error. Retain the original
            // per-attribute reader as the exact fallback in that case.
            if memory.read(start as u32, &mut bytes).is_ok() {
                snapshot.ranges.push((start as u32, bytes));
            }
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
        let first = indices.iter().copied().min().unwrap_or(0);
        let last = indices.iter().copied().max().unwrap_or(0);
        let span = u64::from(last) - u64::from(first) + 1;
        if span <= 1_000_000 && span <= indices.len() as u64 * 4 {
            Self::Dense {
                first,
                slots: vec![u32::MAX; span as usize],
            }
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
