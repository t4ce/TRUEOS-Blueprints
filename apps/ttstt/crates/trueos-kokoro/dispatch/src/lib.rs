#![no_std]
#![deny(unsafe_code)]

//! Typed, fail-closed dispatch boundary for the sealed Kokoro AOT stream.
//!
//! Attribute decoding is kept separate from tensor execution: a record that
//! is not part of the model-specific v1 contract is rejected before any
//! kernel can observe tensor memory.

extern crate alloc;
#[cfg(all(feature = "async-convolution", target_os = "trueos"))]
compile_error!("async-convolution is an Ubuntu experiment; native placement is not implemented");
#[cfg(feature = "async-convolution")]
pub mod async_conv;
#[cfg(feature = "async-graph")]
pub mod async_graph;
#[cfg(feature = "occupancy-trace")]
pub mod occupancy;
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

#[cfg(any(test, feature = "async-convolution", feature = "async-graph"))]
extern crate std;

#[cfg(test)]
mod tests;
