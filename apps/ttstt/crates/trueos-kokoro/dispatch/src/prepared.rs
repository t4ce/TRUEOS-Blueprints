//! Owned CPU layouts of immutable model tensors, prepared once at warmup.
use alloc::vec::Vec;
use trueos_kokoro_aot::{Program, StorageKind};
use trueos_kokoro_lstm::PreparedMatrix;
use trueos_kokoro_memory::constant_tensor;

use crate::{Attributes, DispatchError, decode};

#[derive(Clone, Copy, PartialEq, Eq)]
enum QuantLayout {
    Gemm {
        k: usize,
        n: usize,
    },
    Conv {
        input: usize,
        output: usize,
        kernel: usize,
    },
}

pub(crate) struct QuantWeights {
    tensor: u32,
    layout: QuantLayout,
    pub packed: Vec<i8>,
    pub sums: Vec<i32>,
}

/// Cache owns all packs and borrows only immutable artifact DATA. Entries are
/// deduplicated by source tensor and layout, then indexed directly by operation.
pub struct PreparedWeights<'artifact> {
    artifact_sha256: [u8; 32],
    quant_ops: Vec<Option<usize>>,
    quant: Vec<QuantWeights>,
    lstm_ops: Vec<Option<usize>>,
    lstm: Vec<[PreparedMatrix<'artifact>; 4]>,
}

pub(crate) fn filled<T: Clone>(length: usize, value: T) -> Result<Vec<T>, DispatchError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(length)
        .map_err(|_| DispatchError::Allocation)?;
    values.resize(length, value);
    Ok(values)
}

impl<'artifact> PreparedWeights<'artifact> {
    pub fn new(program: &Program<'artifact>) -> Result<Self, DispatchError> {
        let mut result = Self {
            artifact_sha256: *program.artifact_sha256(),
            quant_ops: filled(program.op_count() as usize, None)?,
            quant: Vec::new(),
            lstm_ops: filled(program.op_count() as usize, None)?,
            lstm: Vec::new(),
        };
        for index in 0..program.op_count() {
            let op = program.op(index).ok_or(DispatchError::MissingAttributes)?;
            let attributes = decode(
                program
                    .op_attributes(op)
                    .ok_or(DispatchError::MissingAttributes)?,
                op.opcode,
            )?;
            let tensor = program.op_input(op, 1);
            let layout = match attributes {
                Attributes::DynamicQuantizedGemm(a) => Some(QuantLayout::Gemm {
                    k: a.k as usize,
                    n: a.n as usize,
                }),
                Attributes::DynamicQuantizedConv1d(a) => Some(QuantLayout::Conv {
                    input: a.input_channels as usize,
                    output: a.output_channels as usize,
                    kernel: a.kernel as usize,
                }),
                _ => None,
            };
            if let Some(layout) = layout {
                let tensor = tensor.ok_or(DispatchError::InvalidArity)?;
                if program
                    .tensor(tensor)
                    .ok_or(DispatchError::InvalidArity)?
                    .storage
                    != StorageKind::Constant
                {
                    continue; // Generic mutable-weight graphs retain the uncached adapter.
                }
                let entry = if let Some(found) = result
                    .quant
                    .iter()
                    .position(|entry| entry.tensor == tensor && entry.layout == layout)
                {
                    found
                } else {
                    let (elements, rows) = match layout {
                        QuantLayout::Gemm { k, n } => (k.checked_mul(n), n),
                        QuantLayout::Conv {
                            input,
                            output,
                            kernel,
                        } => (
                            input
                                .checked_mul(output)
                                .and_then(|v| v.checked_mul(kernel)),
                            output,
                        ),
                    };
                    let mut packed = filled(elements.ok_or(DispatchError::ShapeConversion)?, 0_i8)?;
                    let mut sums = filled(rows, 0_i32)?;
                    match layout {
                        QuantLayout::Gemm { k, n } => trueos_ttstt_cpu::pack_matmul_weights_i8(
                            constant_tensor::<i8>(program, tensor)?,
                            k,
                            n,
                            &mut packed,
                            &mut sums,
                        )?,
                        QuantLayout::Conv {
                            input,
                            output,
                            kernel,
                        } => trueos_ttstt_cpu::pack_conv1d_weights_u8(
                            constant_tensor::<u8>(program, tensor)?,
                            output,
                            input,
                            kernel,
                            &mut packed,
                            &mut sums,
                        )?,
                    }
                    result
                        .quant
                        .try_reserve(1)
                        .map_err(|_| DispatchError::Allocation)?;
                    let found = result.quant.len();
                    result.quant.push(QuantWeights {
                        tensor,
                        layout,
                        packed,
                        sums,
                    });
                    found
                };
                result.quant_ops[index as usize] = Some(entry);
            }
            if let Attributes::BiLstm256(a) = attributes {
                let input_id = program.op_input(op, 1).ok_or(DispatchError::InvalidArity)?;
                let recurrent_id = program.op_input(op, 2).ok_or(DispatchError::InvalidArity)?;
                if [input_id, recurrent_id].into_iter().any(|id| {
                    program
                        .tensor(id)
                        .is_none_or(|t| t.storage != StorageKind::Constant)
                }) {
                    continue;
                }
                let weights = constant_tensor::<f32>(program, input_id)?;
                let recurrent = constant_tensor::<f32>(program, recurrent_id)?;
                let width = a.input_width as usize;
                let half = 1024usize
                    .checked_mul(width)
                    .ok_or(DispatchError::ShapeConversion)?;
                let recurrent_half = 1024 * 256;
                if weights.len() != 2 * half || recurrent.len() != 2 * recurrent_half {
                    return Err(DispatchError::ShapeConversion);
                }
                let matrices = [
                    PreparedMatrix::new(&weights[..half], 1024, width)?,
                    PreparedMatrix::new(&weights[half..], 1024, width)?,
                    PreparedMatrix::new(&recurrent[..recurrent_half], 1024, 256)?,
                    PreparedMatrix::new(&recurrent[recurrent_half..], 1024, 256)?,
                ];
                result
                    .lstm
                    .try_reserve(1)
                    .map_err(|_| DispatchError::Allocation)?;
                result.lstm_ops[index as usize] = Some(result.lstm.len());
                result.lstm.push(matrices);
            }
        }
        Ok(result)
    }

    pub fn packed_bytes(&self) -> usize {
        self.quant
            .iter()
            .map(|p| p.packed.len() + p.sums.len() * 4)
            .sum::<usize>()
            + self
                .lstm
                .iter()
                .flat_map(|p| p.iter())
                .map(|p| p.view().packed().len() * 4)
                .sum::<usize>()
    }

    pub(crate) fn matches(&self, program: &Program<'_>) -> bool {
        self.artifact_sha256 == *program.artifact_sha256()
    }

    pub(crate) fn quant(&self, op: u32) -> Option<&QuantWeights> {
        self.quant
            .get(self.quant_ops.get(op as usize).copied().flatten()?)
    }

    pub(crate) fn lstm(&self, op: u32) -> Option<&[PreparedMatrix<'artifact>; 4]> {
        self.lstm
            .get(self.lstm_ops.get(op as usize).copied().flatten()?)
    }
}
