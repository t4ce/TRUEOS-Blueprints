//! TRUEOS build adapter for Veloren's optional thread-priority hints.

use std::{error, fmt, thread};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadPriority {
    Min,
    Crossplatform(ThreadPriorityValue),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThreadPriorityValue(u8);

impl TryFrom<u8> for ThreadPriorityValue {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> { Ok(Self(value)) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadSchedulePolicy {
    Normal(NormalThreadSchedulePolicy),
    Realtime(RealtimeThreadSchedulePolicy),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NormalThreadSchedulePolicy {
    Batch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RealtimeThreadSchedulePolicy {
    RoundRobin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unsupported;

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("thread priority controls are unsupported on TRUEOS")
    }
}

impl error::Error for Unsupported {}

pub trait ThreadExt {
    fn set_priority(&self, priority: ThreadPriority) -> Result<(), Unsupported>;

    fn set_priority_and_policy(
        &self,
        policy: ThreadSchedulePolicy,
        priority: ThreadPriority,
    ) -> Result<(), Unsupported>;
}

impl ThreadExt for thread::Thread {
    fn set_priority(&self, _priority: ThreadPriority) -> Result<(), Unsupported> {
        Err(Unsupported)
    }

    fn set_priority_and_policy(
        &self,
        _policy: ThreadSchedulePolicy,
        _priority: ThreadPriority,
    ) -> Result<(), Unsupported> {
        Err(Unsupported)
    }
}
