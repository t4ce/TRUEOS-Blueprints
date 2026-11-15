#![no_std]
#![deny(unsafe_code)]

//! Typed, fail-closed dispatch boundary for the sealed Kokoro AOT stream.
//!
//! Attribute decoding is kept separate from tensor execution: a record that
//! is not part of the model-specific v1 contract is rejected before any
//! kernel can observe tensor memory.

extern crate alloc;
pub mod attributes;
mod cpu;
mod prepared;
pub use prepared::PreparedWeights;
#[cfg(feature = "profile-inference")]
mod profiler;
#[cfg(feature = "profile-inference")]
pub use profiler::{ExecutionProfile, OpTiming};

pub use attributes::{
    ATTRIBUTE_ABI_VERSION, AttributeError, Attributes, ContractReason, decode, record_bytes,
};
pub use cpu::{
    CpuDispatchPlan, CpuDispatcher, CpuExecutionState, CpuWorkspace, CpuWorkspaceRequirements, DispatchError,
    KOKORO_CPU_WORKSPACE_REQUIREMENTS, WorkspaceError, native_dispatch_requires_workspace,
    native_dispatch_supported,
};

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;
