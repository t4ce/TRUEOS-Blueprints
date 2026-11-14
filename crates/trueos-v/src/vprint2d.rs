//! Compact producer API for the kernel-owned asynchronous 2D print queue.

extern crate alloc;
use alloc::vec::Vec;

#[path = "vprint2d/a4.rs"]
pub mod a4;

const DOCUMENT_GRIDPAPER_A4: u32 = 1;
const DOCUMENT_GRIDPAPER_REQUEST: u32 = 2;

unsafe extern "C" {
    fn trueos_vlayer_print2d_submit(
        document_kind: u32,
        subject: u64,
        raw_ptr: *const u8,
        raw_len: usize,
    ) -> i64;
    fn trueos_vlayer_print2d_status(job_id: u32) -> i32;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct JobId(u32);

impl JobId {
    pub const fn get(self) -> u32 {
        self.0
    }

    pub fn status(self) -> Result<JobState, Error> {
        status(self)
    }

    pub fn is_done(self) -> Result<bool, Error> {
        self.status().map(JobState::is_terminal)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum JobState {
    Queued = 1,
    WaitingForPrinter = 2,
    Rendering = 3,
    Connecting = 4,
    Sending = 5,
    Submitted = 6,
    Printing = 7,
    Completed = 8,
    Failed = 9,
    Canceled = 10,
    OutcomeUnknown = 11,
}

impl JobState {
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Canceled | Self::OutcomeUnknown
        )
    }

    pub const fn is_done(self) -> bool {
        self.is_terminal()
    }
}

impl TryFrom<i32> for JobState {
    type Error = Error;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Queued),
            2 => Ok(Self::WaitingForPrinter),
            3 => Ok(Self::Rendering),
            4 => Ok(Self::Connecting),
            5 => Ok(Self::Sending),
            6 => Ok(Self::Submitted),
            7 => Ok(Self::Printing),
            8 => Ok(Self::Completed),
            9 => Ok(Self::Failed),
            10 => Ok(Self::Canceled),
            11 => Ok(Self::OutcomeUnknown),
            other => Err(error_from_code(other)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidDocument,
    QueueFull,
    NotOwner,
    UnknownJob,
    Transport,
    Unknown(i32),
}

pub fn submit_gridpaper_a4(generation: u64, raw: &[u8]) -> Result<JobId, Error> {
    submit(DOCUMENT_GRIDPAPER_A4, generation, raw)
}

pub fn submit_gridpaper_request(token: u32) -> Result<JobId, Error> {
    submit(DOCUMENT_GRIDPAPER_REQUEST, u64::from(token), &[])
}

pub fn status(job_id: JobId) -> Result<JobState, Error> {
    let raw = unsafe { trueos_vlayer_print2d_status(job_id.0) };
    JobState::try_from(raw)
}

fn submit(document_kind: u32, subject: u64, raw: &[u8]) -> Result<JobId, Error> {
    let result =
        unsafe { trueos_vlayer_print2d_submit(document_kind, subject, raw.as_ptr(), raw.len()) };
    if result <= 0 || result > u32::MAX as i64 {
        return Err(error_from_code(result as i32));
    }
    Ok(JobId(result as u32))
}

fn error_from_code(code: i32) -> Error {
    match code {
        -1 => Error::InvalidDocument,
        -2 => Error::QueueFull,
        -3 => Error::NotOwner,
        -4 => Error::UnknownJob,
        -5 => Error::Transport,
        other => Error::Unknown(other),
    }
}

/// Submit an app-rendered A4 PWG document through the existing bounded ABI.
/// Uploads are invisible to the spooler until the final commit succeeds.
pub fn submit_a4_pwg(document: &[u8]) -> Result<JobId, Error> {
    if document.is_empty() || document.len() > a4::MAX_DOCUMENT_BYTES {
        return Err(Error::InvalidDocument);
    }
    let mut begin = [0u8; 8];
    begin[..4].copy_from_slice(&1u32.to_le_bytes());
    begin[4..].copy_from_slice(&(document.len() as u32).to_le_bytes());
    let id = submit(3, 0, &begin)?;
    let result = (|| {
        let mut payload = Vec::with_capacity(64 * 1024 + 4);
        let mut offset = 0u32;
        for chunk in document.chunks(64 * 1024) {
            payload.clear();
            payload.extend_from_slice(&offset.to_le_bytes());
            payload.extend_from_slice(chunk);
            let appended = submit(4, u64::from(id.get()), &payload)?;
            if appended != id {
                return Err(Error::Transport);
            }
            offset += chunk.len() as u32;
        }
        let committed = submit(5, u64::from(id.get()), &[])?;
        if committed != id {
            return Err(Error::Transport);
        }
        Ok(id)
    })();
    if result.is_err() {
        let _ = submit(6, u64::from(id.get()), &[]);
    }
    result
}
