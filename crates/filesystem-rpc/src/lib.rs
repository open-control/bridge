//! Borrowed, bounded filesystem wire contract. No transport or operation state.
pub const REQUEST: u8 = 0xfc;
pub const RESPONSE: u8 = 0xfd;
pub const VERSION: u8 = 3;
pub const HEADER: usize = 24;
pub const MAX_BODY: usize = 32_512;
pub const MAX_DEADLINE_MS: u32 = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Operation {
    Capabilities,
    Stat,
    List,
    Read,
    UploadBegin,
    UploadChunk,
    UploadCommit,
    UploadAbort,
    Mkdir,
    Delete,
    Rename,
    ConditionalReplace,
    ConditionalDelete,
    Poll,
    Cancel,
}
impl Operation {
    pub fn decode(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Capabilities,
            1 => Self::Stat,
            2 => Self::List,
            3 => Self::Read,
            4 => Self::UploadBegin,
            5 => Self::UploadChunk,
            6 => Self::UploadCommit,
            7 => Self::UploadAbort,
            8 => Self::Mkdir,
            9 => Self::Delete,
            10 => Self::Rename,
            11 => Self::ConditionalReplace,
            12 => Self::ConditionalDelete,
            13 => Self::Poll,
            14 => Self::Cancel,
            _ => return None,
        })
    }
    pub fn retained(self) -> bool {
        matches!(
            self,
            Self::UploadCommit
                | Self::Mkdir
                | Self::Delete
                | Self::Rename
                | Self::ConditionalReplace
                | Self::ConditionalDelete
        )
    }
    pub fn control(self) -> bool {
        matches!(self, Self::Poll | Self::Cancel)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Error {
    None,
    InvalidMessage,
    InvalidArgument,
    Unsupported,
    NotFound,
    BusyPlaying,
    ResourceExhausted,
    Conflict,
    PreconditionFailed,
    DeadlineExceeded,
    MediaChanged,
    StorageUnavailable,
    StorageReadFailed,
    StorageWriteFailed,
    StorageCorrupt,
    Cancelled,
    Internal,
    ResultExpired,
    CancelTooLate,
}
impl Error {
    pub fn decode(value: u16) -> Option<Self> {
        Some(match value {
            0 => Self::None,
            1 => Self::InvalidMessage,
            2 => Self::InvalidArgument,
            3 => Self::Unsupported,
            4 => Self::NotFound,
            5 => Self::BusyPlaying,
            6 => Self::ResourceExhausted,
            7 => Self::Conflict,
            8 => Self::PreconditionFailed,
            9 => Self::DeadlineExceeded,
            10 => Self::MediaChanged,
            11 => Self::StorageUnavailable,
            12 => Self::StorageReadFailed,
            13 => Self::StorageWriteFailed,
            14 => Self::StorageCorrupt,
            15 => Self::Cancelled,
            16 => Self::Internal,
            17 => Self::ResultExpired,
            18 => Self::CancelTooLate,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum State {
    Request,
    Complete,
    Pending,
    Failed,
    Cancelled,
}
impl State {
    fn decode(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Request,
            1 => Self::Complete,
            2 => Self::Pending,
            3 => Self::Failed,
            4 => Self::Cancelled,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame<'a> {
    pub operation: Operation,
    pub state: State,
    pub request_id: u16,
    pub error: Error,
    pub nonce: u32,
    pub operation_id: u32,
    /// Request deadline or response retry interval, milliseconds.
    pub delay_ms: u32,
    pub body: &'a [u8],
    /// A repeated mutation returned a retained result, rather than a fresh admission.
    pub replayed: bool,
}

impl Frame<'_> {
    pub fn valid(&self) -> bool {
        if self.request_id == 0 || self.body.len() > MAX_BODY {
            return false;
        }
        if self.replayed
            && (self.state == State::Request
                || !self.operation.retained()
                || self.operation_id == 0)
        {
            return false;
        }
        if self.state == State::Request {
            if self.error != Error::None {
                return false;
            }
            if self.operation.control() {
                self.nonce != 0
                    && self.operation_id != 0
                    && self.delay_ms == 0
                    && self.body.is_empty()
            } else if self.operation.retained() {
                self.nonce != 0
                    && self.operation_id == 0
                    && (1..=MAX_DEADLINE_MS).contains(&self.delay_ms)
            } else {
                self.nonce == 0 && self.operation_id == 0 && self.delay_ms == 0
            }
        } else {
            if self.operation.retained() || self.operation.control() {
                if self.nonce == 0 || (self.operation_id == 0 && self.state != State::Failed) {
                    return false;
                }
            } else if self.nonce != 0
                || self.operation_id != 0
                || self.state == State::Pending
                || self.state == State::Cancelled
            {
                return false;
            }
            match self.state {
                State::Complete => self.error == Error::None && self.delay_ms == 0,
                State::Pending => {
                    self.error == Error::None
                        && self.nonce != 0
                        && self.operation_id != 0
                        && (1..=MAX_DEADLINE_MS).contains(&self.delay_ms)
                        && self.body.is_empty()
                }
                State::Failed => {
                    self.error != Error::None
                        && self.error != Error::Cancelled
                        && self.delay_ms == 0
                        && self.body.is_empty()
                }
                State::Cancelled => {
                    self.error == Error::Cancelled
                        && self.nonce != 0
                        && self.operation_id != 0
                        && self.delay_ms == 0
                        && self.body.is_empty()
                }
                State::Request => false,
            }
        }
    }
}

pub fn decode(data: &[u8]) -> Option<Frame<'_>> {
    if data.len() < HEADER || data[1] != VERSION {
        return None;
    }
    let state = State::decode(data[3] & 0x7f)?;
    if data[0]
        != if state == State::Request {
            REQUEST
        } else {
            RESPONSE
        }
    {
        return None;
    }
    let u16_at = |i| u16::from_le_bytes([data[i], data[i + 1]]);
    let u32_at = |i| u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
    let size = u32_at(20) as usize;
    if size > MAX_BODY || data.len() - HEADER != size {
        return None;
    }
    let frame = Frame {
        operation: Operation::decode(data[2])?,
        state,
        request_id: u16_at(4),
        error: Error::decode(u16_at(6))?,
        nonce: u32_at(8),
        operation_id: u32_at(12),
        delay_ms: u32_at(16),
        body: &data[HEADER..],
        replayed: data[3] & 0x80 != 0,
    };
    frame.valid().then_some(frame)
}

/// Invalid requests and insufficient capacity leave the output untouched.
pub fn encode(frame: Frame<'_>, out: &mut [u8]) -> Option<usize> {
    if !frame.valid() || out.len() < HEADER + frame.body.len() {
        return None;
    }
    out[0] = if frame.state == State::Request {
        REQUEST
    } else {
        RESPONSE
    };
    out[1] = VERSION;
    out[2] = frame.operation as u8;
    out[3] = frame.state as u8 | if frame.replayed { 0x80 } else { 0 };
    out[4..6].copy_from_slice(&frame.request_id.to_le_bytes());
    out[6..8].copy_from_slice(&(frame.error as u16).to_le_bytes());
    for (offset, value) in [
        (8, frame.nonce),
        (12, frame.operation_id),
        (16, frame.delay_ms),
        (20, frame.body.len() as u32),
    ] {
        out[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    out[HEADER..HEADER + frame.body.len()].copy_from_slice(frame.body);
    Some(HEADER + frame.body.len())
}
