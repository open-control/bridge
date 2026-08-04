//! Persistence-job protocol v1 shared contract.
//!
//! This module deliberately owns no runtime job state. It defines the fixed
//! controller wire, compatibility mappings and deterministic policy decisions
//! consumed by the Core and Manager successor lots. All production codecs use
//! borrowed input and caller-provided output buffers.

// The additive supplier is intentionally not fully consumed by this binary
// until L-R05-06/L-R05-07. The lease expires when those consumers land.
#![cfg_attr(not(test), allow(dead_code))]

pub const FILESYSTEM_FEATURE_PERSISTENCE_JOBS: u32 = 1 << 4;
pub const REQUEST_MESSAGE_ID: u8 = 0xFC;
pub const RESPONSE_MESSAGE_ID: u8 = 0xFD;
pub const SCHEMA: u8 = 1;
pub const REQUEST_NAME: &[u8] = b"FsJobRequest";
pub const RESPONSE_NAME: &[u8] = b"FsJobResponse";

pub const REQUEST_APPLICATION_HEADER_BYTES: usize = 16;
pub const RESPONSE_APPLICATION_HEADER_BYTES: usize = 20;
pub const CAPABILITIES_BODY_BYTES: usize = 24;
pub const MAX_INNER_REQUEST_BYTES: usize = 32_512;
pub const MAX_INNER_RESPONSE_BYTES: usize = 32_512;
pub const MAX_TOTAL_DEADLINE_MS: u32 = 10_000;
pub const TERMINAL_RETENTION_MS: u32 = 30_000;
pub const MAX_PROGRESS_PER_MILLE: u32 = 1_000;
pub const MAX_CONCURRENT_JOBS: u8 = 2;
pub const BRIDGE_JOB_PROTOCOL_VERSION: u8 = 1;

pub const FEATURE_START: u32 = 1 << 0;
pub const FEATURE_POLL: u32 = 1 << 1;
pub const FEATURE_CANCEL: u32 = 1 << 2;
pub const FEATURE_TERMINAL_RETENTION: u32 = 1 << 3;
pub const FEATURE_TYPED_ERRORS: u32 = 1 << 4;
pub const FEATURE_LEGACY_MAPPING: u32 = 1 << 5;
pub const ALL_FEATURES: u32 = FEATURE_START
    | FEATURE_POLL
    | FEATURE_CANCEL
    | FEATURE_TERMINAL_RETENTION
    | FEATURE_TYPED_ERRORS
    | FEATURE_LEGACY_MAPPING;

pub const FLAG_DUPLICATE_START: u8 = 1 << 0;
pub const FLAG_LEGACY_MAPPED: u8 = 1 << 1;
pub const FLAG_TERMINAL_RETAINED: u8 = 1 << 2;
pub const FLAG_CANCEL_TOO_LATE: u8 = 1 << 3;
pub const ALL_RESPONSE_FLAGS: u8 =
    FLAG_DUPLICATE_START | FLAG_LEGACY_MAPPED | FLAG_TERMINAL_RETAINED | FLAG_CANCEL_TOO_LATE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Command {
    Capabilities = 0,
    Start = 1,
    Poll = 2,
    Cancel = 3,
}

impl Command {
    fn decode(value: u8) -> Result<Self, CodecError> {
        match value {
            0 => Ok(Self::Capabilities),
            1 => Ok(Self::Start),
            2 => Ok(Self::Poll),
            3 => Ok(Self::Cancel),
            _ => Err(CodecError::UnknownCommand),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum JobState {
    None = 0,
    Accepted = 1,
    Pending = 2,
    Completed = 3,
    CancelPending = 4,
    Cancelled = 5,
    Failed = 6,
    Rejected = 7,
}

impl JobState {
    fn decode(value: u8) -> Result<Self, CodecError> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Accepted),
            2 => Ok(Self::Pending),
            3 => Ok(Self::Completed),
            4 => Ok(Self::CancelPending),
            5 => Ok(Self::Cancelled),
            6 => Ok(Self::Failed),
            7 => Ok(Self::Rejected),
            _ => Err(CodecError::UnknownState),
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Cancelled | Self::Failed | Self::Rejected
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum JobError {
    None = 0,
    InvalidMessage = 1,
    InvalidArgument = 2,
    Unsupported = 3,
    NotFound = 4,
    BusyPlaying = 5,
    ResourceExhausted = 6,
    Conflict = 7,
    PreconditionFailed = 8,
    DeadlineExceeded = 9,
    MediaChanged = 10,
    StorageUnavailable = 11,
    StorageReadFailed = 12,
    StorageWriteFailed = 13,
    StorageCorrupt = 14,
    Cancelled = 15,
    Internal = 16,
    LegacyBusy = 17,
    LegacyStorageError = 18,
}

impl JobError {
    fn decode(value: u8) -> Result<Self, CodecError> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::InvalidMessage),
            2 => Ok(Self::InvalidArgument),
            3 => Ok(Self::Unsupported),
            4 => Ok(Self::NotFound),
            5 => Ok(Self::BusyPlaying),
            6 => Ok(Self::ResourceExhausted),
            7 => Ok(Self::Conflict),
            8 => Ok(Self::PreconditionFailed),
            9 => Ok(Self::DeadlineExceeded),
            10 => Ok(Self::MediaChanged),
            11 => Ok(Self::StorageUnavailable),
            12 => Ok(Self::StorageReadFailed),
            13 => Ok(Self::StorageWriteFailed),
            14 => Ok(Self::StorageCorrupt),
            15 => Ok(Self::Cancelled),
            16 => Ok(Self::Internal),
            17 => Ok(Self::LegacyBusy),
            18 => Ok(Self::LegacyStorageError),
            _ => Err(CodecError::UnknownError),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LegacyStatus {
    Ok = 0,
    InvalidMessage = 1,
    InvalidArgument = 2,
    NotFound = 3,
    Busy = 4,
    TooLarge = 5,
    StorageError = 6,
    InvalidState = 7,
    Unsupported = 8,
    PreconditionFailed = 9,
}

impl LegacyStatus {
    pub fn decode(value: u8) -> Result<Self, CodecError> {
        match value {
            0 => Ok(Self::Ok),
            1 => Ok(Self::InvalidMessage),
            2 => Ok(Self::InvalidArgument),
            3 => Ok(Self::NotFound),
            4 => Ok(Self::Busy),
            5 => Ok(Self::TooLarge),
            6 => Ok(Self::StorageError),
            7 => Ok(Self::InvalidState),
            8 => Ok(Self::Unsupported),
            9 => Ok(Self::PreconditionFailed),
            _ => Err(CodecError::UnknownLegacyStatus),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecError {
    BufferTooSmall,
    Truncated,
    InvalidMessageId,
    InvalidMessageName,
    UnsupportedSchema,
    UnknownCommand,
    UnknownState,
    UnknownError,
    UnknownLegacyStatus,
    InvalidFlags,
    InvalidReserved,
    InvalidIdentity,
    InvalidDeadline,
    InvalidBody,
    InvalidProgress,
    LimitExceeded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobRequest<'a> {
    pub request_id: u16,
    pub command: Command,
    pub client_nonce: u32,
    pub job_id: u32,
    pub total_deadline_ms: u32,
    pub inner_request: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobResponse<'a> {
    pub request_id: u16,
    pub command: Command,
    pub state: JobState,
    pub error: JobError,
    pub flags: u8,
    pub client_nonce: u32,
    pub job_id: u32,
    pub retry_after_ms: u32,
    pub progress_per_mille: u32,
    pub body: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub protocol_version: u8,
    pub max_concurrent_jobs: u8,
    pub feature_flags: u32,
    pub max_inner_request_bytes: u32,
    pub max_inner_response_bytes: u32,
    pub max_total_deadline_ms: u32,
    pub terminal_retention_ms: u32,
}

impl Capabilities {
    pub const V1: Self = Self {
        protocol_version: BRIDGE_JOB_PROTOCOL_VERSION,
        max_concurrent_jobs: MAX_CONCURRENT_JOBS,
        feature_flags: ALL_FEATURES,
        max_inner_request_bytes: MAX_INNER_REQUEST_BYTES as u32,
        max_inner_response_bytes: MAX_INNER_RESPONSE_BYTES as u32,
        max_total_deadline_ms: MAX_TOTAL_DEADLINE_MS,
        terminal_retention_ms: TERMINAL_RETENTION_MS,
    };

    pub fn encode(self, out: &mut [u8]) -> Result<usize, CodecError> {
        validate_capabilities(self)?;
        let mut writer = Writer::new(out);
        writer.u8(self.protocol_version)?;
        writer.u8(self.max_concurrent_jobs)?;
        writer.u16(0)?;
        writer.u32(self.feature_flags)?;
        writer.u32(self.max_inner_request_bytes)?;
        writer.u32(self.max_inner_response_bytes)?;
        writer.u32(self.max_total_deadline_ms)?;
        writer.u32(self.terminal_retention_ms)?;
        Ok(writer.position())
    }

    pub fn decode(data: &[u8]) -> Result<Self, CodecError> {
        if data.len() != CAPABILITIES_BODY_BYTES {
            return Err(CodecError::InvalidBody);
        }
        let mut reader = Reader::new(data);
        let capabilities = Self {
            protocol_version: reader.u8()?,
            max_concurrent_jobs: reader.u8()?,
            feature_flags: {
                if reader.u16()? != 0 {
                    return Err(CodecError::InvalidReserved);
                }
                reader.u32()?
            },
            max_inner_request_bytes: reader.u32()?,
            max_inner_response_bytes: reader.u32()?,
            max_total_deadline_ms: reader.u32()?,
            terminal_retention_ms: reader.u32()?,
        };
        validate_capabilities(capabilities)?;
        Ok(capabilities)
    }
}

pub fn encode_request(request: JobRequest<'_>, out: &mut [u8]) -> Result<usize, CodecError> {
    validate_request(request)?;
    let mut writer = Writer::new(out);
    write_envelope(
        &mut writer,
        REQUEST_MESSAGE_ID,
        REQUEST_NAME,
        request.request_id,
    )?;
    writer.u8(request.command as u8)?;
    writer.u8(0)?;
    writer.u16(0)?;
    writer.u32(request.client_nonce)?;
    writer.u32(request.job_id)?;
    writer.u32(request.total_deadline_ms)?;
    writer.bytes(request.inner_request)?;
    Ok(writer.position())
}

pub fn decode_request(data: &[u8]) -> Result<JobRequest<'_>, CodecError> {
    let (request_id, mut reader) = read_envelope(data, REQUEST_MESSAGE_ID, REQUEST_NAME)?;
    let command = Command::decode(reader.u8()?)?;
    if reader.u8()? != 0 {
        return Err(CodecError::InvalidFlags);
    }
    if reader.u16()? != 0 {
        return Err(CodecError::InvalidReserved);
    }
    let request = JobRequest {
        request_id,
        command,
        client_nonce: reader.u32()?,
        job_id: reader.u32()?,
        total_deadline_ms: reader.u32()?,
        inner_request: reader.rest(),
    };
    validate_request(request)?;
    Ok(request)
}

pub fn encode_response(response: JobResponse<'_>, out: &mut [u8]) -> Result<usize, CodecError> {
    validate_response(response)?;
    let mut writer = Writer::new(out);
    write_envelope(
        &mut writer,
        RESPONSE_MESSAGE_ID,
        RESPONSE_NAME,
        response.request_id,
    )?;
    writer.u8(response.command as u8)?;
    writer.u8(response.state as u8)?;
    writer.u8(response.error as u8)?;
    writer.u8(response.flags)?;
    writer.u32(response.client_nonce)?;
    writer.u32(response.job_id)?;
    writer.u32(response.retry_after_ms)?;
    writer.u32(response.progress_per_mille)?;
    writer.bytes(response.body)?;
    Ok(writer.position())
}

pub fn decode_response(data: &[u8]) -> Result<JobResponse<'_>, CodecError> {
    let (request_id, mut reader) = read_envelope(data, RESPONSE_MESSAGE_ID, RESPONSE_NAME)?;
    let response = JobResponse {
        request_id,
        command: Command::decode(reader.u8()?)?,
        state: JobState::decode(reader.u8()?)?,
        error: JobError::decode(reader.u8()?)?,
        flags: reader.u8()?,
        client_nonce: reader.u32()?,
        job_id: reader.u32()?,
        retry_after_ms: reader.u32()?,
        progress_per_mille: reader.u32()?,
        body: reader.rest(),
    };
    validate_response(response)?;
    Ok(response)
}

/// Returns true for the reserved job response before any schema parsing.
///
/// The session uses this after pending-RPC matching so a valid waiter wins and
/// every unmatched or expired job response is quarantined.
pub fn is_reserved_job_response(data: &[u8]) -> bool {
    data.first().copied() == Some(RESPONSE_MESSAGE_ID)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartDisposition {
    NewNonce,
    Duplicate,
    Conflict,
}

/// Classifies nonce reuse without retaining or allocating any data.
pub fn classify_start(
    retained_nonce: u32,
    retained_inner_request: &[u8],
    requested_nonce: u32,
    requested_inner_request: &[u8],
) -> StartDisposition {
    if retained_nonce != requested_nonce {
        StartDisposition::NewNonce
    } else if retained_inner_request == requested_inner_request {
        StartDisposition::Duplicate
    } else {
        StartDisposition::Conflict
    }
}

/// Uses rollover-safe elapsed arithmetic. Retention values must stay below
/// half the `u32` clock range; the v1 value is 30 seconds.
pub fn terminal_is_retained(now_ms: u32, terminal_at_ms: u32, retention_ms: u32) -> bool {
    retention_ms <= i32::MAX as u32 && now_ms.wrapping_sub(terminal_at_ms) <= retention_ms
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelBoundary {
    SafeCheckpoint,
    Irreversible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelDisposition {
    RequestCancellation,
    AlreadyRequested,
    AlreadyTerminal,
    TooLate,
}

pub fn classify_cancel(state: JobState, boundary: CancelBoundary) -> CancelDisposition {
    if state.is_terminal() || state == JobState::None {
        CancelDisposition::AlreadyTerminal
    } else if state == JobState::CancelPending {
        CancelDisposition::AlreadyRequested
    } else if boundary == CancelBoundary::Irreversible {
        CancelDisposition::TooLate
    } else {
        CancelDisposition::RequestCancellation
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NegotiatedMode {
    LegacyStoppedOnly,
    PersistenceJobsV1,
}

/// Both the Core feature and the Bridge quarantine supplier are required.
pub fn negotiate_mode(
    core_filesystem_features: u32,
    bridge_protocol_version: u8,
) -> NegotiatedMode {
    if core_filesystem_features & FILESYSTEM_FEATURE_PERSISTENCE_JOBS != 0
        && bridge_protocol_version >= BRIDGE_JOB_PROTOCOL_VERSION
    {
        NegotiatedMode::PersistenceJobsV1
    } else {
        NegotiatedMode::LegacyStoppedOnly
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyProjection {
    pub status: LegacyStatus,
    pub lossy: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypedProjection {
    pub error: JobError,
    pub lossy: bool,
}

pub const fn error_from_legacy(status: LegacyStatus) -> TypedProjection {
    match status {
        LegacyStatus::Ok => TypedProjection {
            error: JobError::None,
            lossy: false,
        },
        LegacyStatus::InvalidMessage => TypedProjection {
            error: JobError::InvalidMessage,
            lossy: false,
        },
        LegacyStatus::InvalidArgument => TypedProjection {
            error: JobError::InvalidArgument,
            lossy: false,
        },
        LegacyStatus::NotFound => TypedProjection {
            error: JobError::NotFound,
            lossy: false,
        },
        LegacyStatus::Busy => TypedProjection {
            error: JobError::LegacyBusy,
            lossy: true,
        },
        LegacyStatus::TooLarge => TypedProjection {
            error: JobError::ResourceExhausted,
            lossy: true,
        },
        LegacyStatus::StorageError => TypedProjection {
            error: JobError::LegacyStorageError,
            lossy: true,
        },
        LegacyStatus::InvalidState => TypedProjection {
            error: JobError::Internal,
            lossy: true,
        },
        LegacyStatus::Unsupported => TypedProjection {
            error: JobError::Unsupported,
            lossy: false,
        },
        LegacyStatus::PreconditionFailed => TypedProjection {
            error: JobError::PreconditionFailed,
            lossy: false,
        },
    }
}

pub const fn error_to_legacy(error: JobError) -> LegacyProjection {
    match error {
        JobError::None => LegacyProjection {
            status: LegacyStatus::Ok,
            lossy: false,
        },
        JobError::InvalidMessage => LegacyProjection {
            status: LegacyStatus::InvalidMessage,
            lossy: false,
        },
        JobError::InvalidArgument => LegacyProjection {
            status: LegacyStatus::InvalidArgument,
            lossy: false,
        },
        JobError::Unsupported => LegacyProjection {
            status: LegacyStatus::Unsupported,
            lossy: false,
        },
        JobError::NotFound => LegacyProjection {
            status: LegacyStatus::NotFound,
            lossy: false,
        },
        JobError::PreconditionFailed => LegacyProjection {
            status: LegacyStatus::PreconditionFailed,
            lossy: false,
        },
        JobError::BusyPlaying | JobError::LegacyBusy => LegacyProjection {
            status: LegacyStatus::Busy,
            lossy: true,
        },
        JobError::ResourceExhausted => LegacyProjection {
            status: LegacyStatus::TooLarge,
            lossy: true,
        },
        JobError::MediaChanged
        | JobError::StorageUnavailable
        | JobError::StorageReadFailed
        | JobError::StorageWriteFailed
        | JobError::StorageCorrupt
        | JobError::LegacyStorageError => LegacyProjection {
            status: LegacyStatus::StorageError,
            lossy: true,
        },
        JobError::Conflict
        | JobError::DeadlineExceeded
        | JobError::Cancelled
        | JobError::Internal => LegacyProjection {
            status: LegacyStatus::InvalidState,
            lossy: true,
        },
    }
}

fn validate_request(request: JobRequest<'_>) -> Result<(), CodecError> {
    match request.command {
        Command::Capabilities => {
            if request.client_nonce != 0
                || request.job_id != 0
                || request.total_deadline_ms != 0
                || !request.inner_request.is_empty()
            {
                return Err(CodecError::InvalidBody);
            }
        }
        Command::Start => {
            if request.client_nonce == 0 || request.job_id != 0 {
                return Err(CodecError::InvalidIdentity);
            }
            if request.total_deadline_ms == 0 || request.total_deadline_ms > MAX_TOTAL_DEADLINE_MS {
                return Err(CodecError::InvalidDeadline);
            }
            if request.inner_request.len() > MAX_INNER_REQUEST_BYTES {
                return Err(CodecError::LimitExceeded);
            }
            if !is_legacy_request_frame(request.inner_request) {
                return Err(CodecError::InvalidBody);
            }
        }
        Command::Poll | Command::Cancel => {
            if request.client_nonce == 0 || request.job_id == 0 {
                return Err(CodecError::InvalidIdentity);
            }
            if request.total_deadline_ms != 0 {
                return Err(CodecError::InvalidDeadline);
            }
            if !request.inner_request.is_empty() {
                return Err(CodecError::InvalidBody);
            }
        }
    }
    Ok(())
}

fn validate_response(response: JobResponse<'_>) -> Result<(), CodecError> {
    if response.flags & !ALL_RESPONSE_FLAGS != 0 {
        return Err(CodecError::InvalidFlags);
    }
    if response.progress_per_mille > MAX_PROGRESS_PER_MILLE {
        return Err(CodecError::InvalidProgress);
    }

    if response.command == Command::Capabilities {
        if response.state != JobState::None
            || response.error != JobError::None
            || response.flags != 0
            || response.client_nonce != 0
            || response.job_id != 0
            || response.retry_after_ms != 0
            || response.progress_per_mille != 0
        {
            return Err(CodecError::InvalidBody);
        }
        Capabilities::decode(response.body)?;
        return Ok(());
    }

    if response.client_nonce == 0 {
        return Err(CodecError::InvalidIdentity);
    }
    if response.command != Command::Start && response.job_id == 0 {
        return Err(CodecError::InvalidIdentity);
    }
    if response.command == Command::Start
        && response.job_id == 0
        && (response.state != JobState::Rejected || response.error == JobError::Conflict)
    {
        return Err(CodecError::InvalidIdentity);
    }

    match response.state {
        JobState::None => return Err(CodecError::InvalidBody),
        JobState::Accepted | JobState::Pending | JobState::CancelPending | JobState::Completed => {
            if response.error != JobError::None {
                return Err(CodecError::InvalidBody);
            }
        }
        JobState::Cancelled => {
            if response.error != JobError::Cancelled {
                return Err(CodecError::InvalidBody);
            }
        }
        JobState::Failed | JobState::Rejected => {
            if response.error == JobError::None {
                return Err(CodecError::InvalidBody);
            }
        }
    }

    if matches!(
        response.state,
        JobState::Accepted | JobState::Pending | JobState::CancelPending
    ) {
        if response.retry_after_ms > MAX_TOTAL_DEADLINE_MS {
            return Err(CodecError::InvalidDeadline);
        }
    } else if response.retry_after_ms != 0 {
        return Err(CodecError::InvalidDeadline);
    }

    if response.state == JobState::Completed {
        if response.body.len() > MAX_INNER_RESPONSE_BYTES {
            return Err(CodecError::LimitExceeded);
        }
        if !is_legacy_response_frame(response.body) {
            return Err(CodecError::InvalidBody);
        }
    } else if !response.body.is_empty() {
        return Err(CodecError::InvalidBody);
    }

    if response.flags & FLAG_DUPLICATE_START != 0
        && (response.command != Command::Start
            || response.job_id == 0
            || response.state == JobState::Rejected)
    {
        return Err(CodecError::InvalidFlags);
    }
    if response.flags & FLAG_TERMINAL_RETAINED != 0
        && (response.command != Command::Poll || !response.state.is_terminal())
    {
        return Err(CodecError::InvalidFlags);
    }
    if response.flags & FLAG_CANCEL_TOO_LATE != 0
        && (response.command != Command::Cancel
            || !matches!(
                response.state,
                JobState::Pending | JobState::Completed | JobState::Failed
            ))
    {
        return Err(CodecError::InvalidFlags);
    }
    if matches!(
        response.error,
        JobError::LegacyBusy | JobError::LegacyStorageError
    ) && response.flags & FLAG_LEGACY_MAPPED == 0
    {
        return Err(CodecError::InvalidFlags);
    }
    if matches!(response.state, JobState::None | JobState::Rejected)
        && response.progress_per_mille != 0
    {
        return Err(CodecError::InvalidProgress);
    }

    Ok(())
}

fn validate_capabilities(capabilities: Capabilities) -> Result<(), CodecError> {
    if capabilities.protocol_version != BRIDGE_JOB_PROTOCOL_VERSION
        || capabilities.max_concurrent_jobs != MAX_CONCURRENT_JOBS
    {
        return Err(CodecError::UnsupportedSchema);
    }
    if capabilities.feature_flags != ALL_FEATURES {
        return Err(CodecError::InvalidFlags);
    }
    if capabilities.max_inner_request_bytes != MAX_INNER_REQUEST_BYTES as u32
        || capabilities.max_inner_response_bytes != MAX_INNER_RESPONSE_BYTES as u32
        || capabilities.max_total_deadline_ms != MAX_TOTAL_DEADLINE_MS
        || capabilities.terminal_retention_ms != TERMINAL_RETENTION_MS
    {
        return Err(CodecError::InvalidBody);
    }
    Ok(())
}

fn write_envelope(
    writer: &mut Writer<'_>,
    message_id: u8,
    name: &[u8],
    request_id: u16,
) -> Result<(), CodecError> {
    writer.u8(message_id)?;
    writer.u8(name.len() as u8)?;
    writer.bytes(name)?;
    writer.u8(SCHEMA)?;
    writer.u16(request_id)?;
    Ok(())
}

fn read_envelope<'a>(
    data: &'a [u8],
    expected_message_id: u8,
    expected_name: &[u8],
) -> Result<(u16, Reader<'a>), CodecError> {
    let mut reader = Reader::new(data);
    if reader.u8()? != expected_message_id {
        return Err(CodecError::InvalidMessageId);
    }
    let name_length = reader.u8()? as usize;
    if reader.bytes(name_length)? != expected_name {
        return Err(CodecError::InvalidMessageName);
    }
    if reader.u8()? != SCHEMA {
        return Err(CodecError::UnsupportedSchema);
    }
    let request_id = reader.u16()?;
    Ok((request_id, reader))
}

fn is_legacy_request_frame(data: &[u8]) -> bool {
    is_legacy_frame(data, is_legacy_request_id)
}

fn is_legacy_response_frame(data: &[u8]) -> bool {
    is_legacy_frame(data, is_legacy_response_id)
}

fn is_legacy_frame(data: &[u8], valid_id: fn(u8) -> bool) -> bool {
    if data.len() < 5 || !valid_id(data[0]) {
        return false;
    }
    let name_length = data[1] as usize;
    let schema_offset = 2usize.saturating_add(name_length);
    let header_bytes = schema_offset.saturating_add(3);
    header_bytes <= data.len() && data[schema_offset] == SCHEMA
}

fn is_legacy_request_id(value: u8) -> bool {
    matches!(
        value,
        0xE0 | 0xE2 | 0xE4 | 0xE6 | 0xE8 | 0xEA | 0xEC | 0xF0 | 0xF2 | 0xF4 | 0xF6 | 0xF8 | 0xFA
    )
}

fn is_legacy_response_id(value: u8) -> bool {
    matches!(
        value,
        0xE1 | 0xE3
            | 0xE5
            | 0xE7
            | 0xE9
            | 0xEB
            | 0xED
            | 0xEF
            | 0xF1
            | 0xF3
            | 0xF5
            | 0xF7
            | 0xF9
            | 0xFB
    )
}

struct Writer<'a> {
    data: &'a mut [u8],
    position: usize,
}

impl<'a> Writer<'a> {
    fn new(data: &'a mut [u8]) -> Self {
        Self { data, position: 0 }
    }

    fn position(&self) -> usize {
        self.position
    }

    fn u8(&mut self, value: u8) -> Result<(), CodecError> {
        if self.position >= self.data.len() {
            return Err(CodecError::BufferTooSmall);
        }
        self.data[self.position] = value;
        self.position += 1;
        Ok(())
    }

    fn u16(&mut self, value: u16) -> Result<(), CodecError> {
        self.bytes(&value.to_le_bytes())
    }

    fn u32(&mut self, value: u32) -> Result<(), CodecError> {
        self.bytes(&value.to_le_bytes())
    }

    fn bytes(&mut self, value: &[u8]) -> Result<(), CodecError> {
        let Some(end) = self.position.checked_add(value.len()) else {
            return Err(CodecError::BufferTooSmall);
        };
        if end > self.data.len() {
            return Err(CodecError::BufferTooSmall);
        }
        self.data[self.position..end].copy_from_slice(value);
        self.position = end;
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Reader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    fn u8(&mut self) -> Result<u8, CodecError> {
        let bytes = self.bytes(1)?;
        Ok(bytes[0])
    }

    fn u16(&mut self) -> Result<u16, CodecError> {
        let bytes = self.bytes(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, CodecError> {
        let bytes = self.bytes(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn bytes(&mut self, count: usize) -> Result<&'a [u8], CodecError> {
        let Some(end) = self.position.checked_add(count) else {
            return Err(CodecError::Truncated);
        };
        if end > self.data.len() {
            return Err(CodecError::Truncated);
        }
        let result = &self.data[self.position..end];
        self.position = end;
        Ok(result)
    }

    fn rest(&self) -> &'a [u8] {
        &self.data[self.position..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_request() -> Vec<u8> {
        vec![0xF2, 0, SCHEMA, 0x34, 0x12, 0]
    }

    fn legacy_response() -> Vec<u8> {
        vec![0xF3, 0, SCHEMA, 0x34, 0x12, LegacyStatus::Ok as u8]
    }

    fn encode_capabilities_body() -> [u8; CAPABILITIES_BODY_BYTES] {
        let mut body = [0u8; CAPABILITIES_BODY_BYTES];
        assert_eq!(Capabilities::V1.encode(&mut body).unwrap(), body.len());
        body
    }

    #[test]
    fn wire_constants_are_exact() {
        assert_eq!(REQUEST_MESSAGE_ID, 0xFC);
        assert_eq!(RESPONSE_MESSAGE_ID, 0xFD);
        assert_eq!(REQUEST_APPLICATION_HEADER_BYTES, 16);
        assert_eq!(RESPONSE_APPLICATION_HEADER_BYTES, 20);
        assert_eq!(CAPABILITIES_BODY_BYTES, 24);
        assert_eq!(REQUEST_NAME.len(), 12);
        assert_eq!(RESPONSE_NAME.len(), 13);
        assert_eq!(ALL_FEATURES, 0x3F);
        assert_eq!(ALL_RESPONSE_FLAGS, 0x0F);
    }

    #[test]
    fn capabilities_request_has_exact_golden_bytes() {
        let request = JobRequest {
            request_id: 0x1234,
            command: Command::Capabilities,
            client_nonce: 0,
            job_id: 0,
            total_deadline_ms: 0,
            inner_request: &[],
        };
        let mut encoded = [0xAA; 64];
        let length = encode_request(request, &mut encoded).unwrap();

        let mut expected = vec![REQUEST_MESSAGE_ID, REQUEST_NAME.len() as u8];
        expected.extend_from_slice(REQUEST_NAME);
        expected.extend_from_slice(&[SCHEMA, 0x34, 0x12]);
        expected.extend_from_slice(&[0; REQUEST_APPLICATION_HEADER_BYTES]);
        assert_eq!(&encoded[..length], expected);
        assert_eq!(decode_request(&encoded[..length]).unwrap(), request);
    }

    #[test]
    fn start_request_roundtrips_exact_identity_and_inner_frame() {
        let inner = legacy_request();
        let request = JobRequest {
            request_id: 7,
            command: Command::Start,
            client_nonce: 0x1020_3040,
            job_id: 0,
            total_deadline_ms: MAX_TOTAL_DEADLINE_MS,
            inner_request: &inner,
        };
        let mut encoded = [0u8; 128];
        let length = encode_request(request, &mut encoded).unwrap();
        let decoded = decode_request(&encoded[..length]).unwrap();
        assert_eq!(decoded, request);
        assert_eq!(
            decoded.inner_request.as_ptr(),
            encoded[length - inner.len()..].as_ptr()
        );
    }

    #[test]
    fn request_validation_rejects_ambiguous_forms() {
        let inner = legacy_request();
        let start = JobRequest {
            request_id: 1,
            command: Command::Start,
            client_nonce: 1,
            job_id: 0,
            total_deadline_ms: 1,
            inner_request: &inner,
        };
        let mut out = [0u8; 128];
        assert_eq!(
            encode_request(
                JobRequest {
                    client_nonce: 0,
                    ..start
                },
                &mut out
            ),
            Err(CodecError::InvalidIdentity)
        );
        assert_eq!(
            encode_request(JobRequest { job_id: 1, ..start }, &mut out),
            Err(CodecError::InvalidIdentity)
        );
        assert_eq!(
            encode_request(
                JobRequest {
                    total_deadline_ms: 0,
                    ..start
                },
                &mut out
            ),
            Err(CodecError::InvalidDeadline)
        );
        assert_eq!(
            encode_request(
                JobRequest {
                    inner_request: &[],
                    ..start
                },
                &mut out
            ),
            Err(CodecError::InvalidBody)
        );

        let poll = JobRequest {
            request_id: 2,
            command: Command::Poll,
            client_nonce: 1,
            job_id: 2,
            total_deadline_ms: 0,
            inner_request: &[],
        };
        assert!(encode_request(poll, &mut out).is_ok());
        assert_eq!(
            encode_request(JobRequest { job_id: 0, ..poll }, &mut out),
            Err(CodecError::InvalidIdentity)
        );
        assert_eq!(
            encode_request(
                JobRequest {
                    inner_request: &inner,
                    ..poll
                },
                &mut out
            ),
            Err(CodecError::InvalidBody)
        );
    }

    #[test]
    fn request_decoder_rejects_unknown_and_reserved_values() {
        let request = JobRequest {
            request_id: 3,
            command: Command::Poll,
            client_nonce: 1,
            job_id: 2,
            total_deadline_ms: 0,
            inner_request: &[],
        };
        let mut encoded = [0u8; 64];
        let length = encode_request(request, &mut encoded).unwrap();
        let application = 1 + 1 + REQUEST_NAME.len() + 1 + 2;

        let mut bad = encoded[..length].to_vec();
        bad[application] = 0xFF;
        assert_eq!(decode_request(&bad), Err(CodecError::UnknownCommand));

        let mut bad = encoded[..length].to_vec();
        bad[application + 1] = 1;
        assert_eq!(decode_request(&bad), Err(CodecError::InvalidFlags));

        let mut bad = encoded[..length].to_vec();
        bad[application + 2] = 1;
        assert_eq!(decode_request(&bad), Err(CodecError::InvalidReserved));

        let mut bad = encoded[..length].to_vec();
        bad[0] = RESPONSE_MESSAGE_ID;
        assert_eq!(decode_request(&bad), Err(CodecError::InvalidMessageId));

        for truncated in 0..length {
            assert!(decode_request(&encoded[..truncated]).is_err());
        }
    }

    #[test]
    fn capabilities_response_has_exact_golden_body_and_roundtrips() {
        let body = encode_capabilities_body();
        assert_eq!(
            body,
            [
                1, 2, 0, 0, 0x3F, 0, 0, 0, 0, 0x7F, 0, 0, 0, 0x7F, 0, 0, 0x10, 0x27, 0, 0, 0x30,
                0x75, 0, 0,
            ]
        );

        let response = JobResponse {
            request_id: 9,
            command: Command::Capabilities,
            state: JobState::None,
            error: JobError::None,
            flags: 0,
            client_nonce: 0,
            job_id: 0,
            retry_after_ms: 0,
            progress_per_mille: 0,
            body: &body,
        };
        let mut encoded = [0u8; 96];
        let length = encode_response(response, &mut encoded).unwrap();
        assert_eq!(decode_response(&encoded[..length]).unwrap(), response);
        assert_eq!(Capabilities::decode(&body).unwrap(), Capabilities::V1);
    }

    #[test]
    fn completed_response_roundtrips_terminal_payload() {
        let body = legacy_response();
        let response = JobResponse {
            request_id: 4,
            command: Command::Poll,
            state: JobState::Completed,
            error: JobError::None,
            flags: FLAG_TERMINAL_RETAINED,
            client_nonce: 0xABCD,
            job_id: 42,
            retry_after_ms: 0,
            progress_per_mille: 1_000,
            body: &body,
        };
        let mut encoded = [0u8; 96];
        let length = encode_response(response, &mut encoded).unwrap();
        assert_eq!(decode_response(&encoded[..length]).unwrap(), response);
    }

    #[test]
    fn response_validation_rejects_inconsistent_state_and_flags() {
        let pending = JobResponse {
            request_id: 1,
            command: Command::Poll,
            state: JobState::Pending,
            error: JobError::None,
            flags: 0,
            client_nonce: 1,
            job_id: 2,
            retry_after_ms: 10,
            progress_per_mille: 500,
            body: &[],
        };
        let mut out = [0u8; 96];
        assert!(encode_response(pending, &mut out).is_ok());
        assert_eq!(
            encode_response(
                JobResponse {
                    client_nonce: 0,
                    ..pending
                },
                &mut out
            ),
            Err(CodecError::InvalidIdentity)
        );
        assert_eq!(
            encode_response(
                JobResponse {
                    progress_per_mille: 1_001,
                    ..pending
                },
                &mut out
            ),
            Err(CodecError::InvalidProgress)
        );
        assert_eq!(
            encode_response(
                JobResponse {
                    flags: FLAG_DUPLICATE_START,
                    ..pending
                },
                &mut out
            ),
            Err(CodecError::InvalidFlags)
        );
        assert_eq!(
            encode_response(
                JobResponse {
                    state: JobState::Failed,
                    ..pending
                },
                &mut out
            ),
            Err(CodecError::InvalidBody)
        );
        assert_eq!(
            encode_response(
                JobResponse {
                    body: &[1],
                    ..pending
                },
                &mut out
            ),
            Err(CodecError::InvalidBody)
        );
    }

    #[test]
    fn response_decoder_rejects_unknown_wire_values() {
        let pending = JobResponse {
            request_id: 1,
            command: Command::Poll,
            state: JobState::Pending,
            error: JobError::None,
            flags: 0,
            client_nonce: 1,
            job_id: 2,
            retry_after_ms: 10,
            progress_per_mille: 500,
            body: &[],
        };
        let mut encoded = [0u8; 96];
        let length = encode_response(pending, &mut encoded).unwrap();
        let application = 1 + 1 + RESPONSE_NAME.len() + 1 + 2;

        let mut bad = encoded[..length].to_vec();
        bad[application + 1] = 0xFF;
        assert_eq!(decode_response(&bad), Err(CodecError::UnknownState));

        let mut bad = encoded[..length].to_vec();
        bad[application + 2] = 0xFF;
        assert_eq!(decode_response(&bad), Err(CodecError::UnknownError));

        let mut bad = encoded[..length].to_vec();
        bad[application + 3] = 0x80;
        assert_eq!(decode_response(&bad), Err(CodecError::InvalidFlags));

        let mut bad = encoded[..length].to_vec();
        bad[2] = b'X';
        assert_eq!(decode_response(&bad), Err(CodecError::InvalidMessageName));
    }

    #[test]
    fn duplicate_and_legacy_flags_require_exact_context() {
        let duplicate = JobResponse {
            request_id: 1,
            command: Command::Start,
            state: JobState::Pending,
            error: JobError::None,
            flags: FLAG_DUPLICATE_START,
            client_nonce: 1,
            job_id: 2,
            retry_after_ms: 10,
            progress_per_mille: 100,
            body: &[],
        };
        let mut out = [0u8; 96];
        assert!(encode_response(duplicate, &mut out).is_ok());
        assert_eq!(
            encode_response(
                JobResponse {
                    state: JobState::Rejected,
                    error: JobError::ResourceExhausted,
                    job_id: 0,
                    retry_after_ms: 0,
                    progress_per_mille: 0,
                    ..duplicate
                },
                &mut out
            ),
            Err(CodecError::InvalidFlags)
        );

        let legacy = JobResponse {
            state: JobState::Failed,
            error: JobError::LegacyStorageError,
            flags: FLAG_LEGACY_MAPPED,
            retry_after_ms: 0,
            progress_per_mille: 100,
            ..duplicate
        };
        assert!(encode_response(legacy, &mut out).is_ok());
        assert_eq!(
            encode_response(JobResponse { flags: 0, ..legacy }, &mut out),
            Err(CodecError::InvalidFlags)
        );
    }

    #[test]
    fn capabilities_reject_reserved_and_future_v1_bits() {
        let body = encode_capabilities_body();
        let mut reserved = body;
        reserved[2] = 1;
        assert_eq!(
            Capabilities::decode(&reserved),
            Err(CodecError::InvalidReserved)
        );

        let mut future = body;
        future[4] |= 0x40;
        assert_eq!(Capabilities::decode(&future), Err(CodecError::InvalidFlags));
    }

    #[test]
    fn start_duplicate_and_conflict_are_deterministic() {
        let retained = legacy_request();
        assert_eq!(
            classify_start(7, &retained, 8, &retained),
            StartDisposition::NewNonce
        );
        assert_eq!(
            classify_start(7, &retained, 7, &retained),
            StartDisposition::Duplicate
        );
        let mut different = retained.clone();
        different.push(1);
        assert_eq!(
            classify_start(7, &retained, 7, &different),
            StartDisposition::Conflict
        );
    }

    #[test]
    fn terminal_retention_is_inclusive_and_rollover_safe() {
        assert!(terminal_is_retained(40_000, 10_000, TERMINAL_RETENTION_MS));
        assert!(!terminal_is_retained(40_001, 10_000, TERMINAL_RETENTION_MS));
        assert!(terminal_is_retained(5, u32::MAX - 4, 10));
        assert!(!terminal_is_retained(6, u32::MAX - 4, 10));
        assert!(!terminal_is_retained(0, 0, i32::MAX as u32 + 1));
    }

    #[test]
    fn cancellation_respects_safe_and_irreversible_boundaries() {
        assert_eq!(
            classify_cancel(JobState::Pending, CancelBoundary::SafeCheckpoint),
            CancelDisposition::RequestCancellation
        );
        assert_eq!(
            classify_cancel(JobState::Pending, CancelBoundary::Irreversible),
            CancelDisposition::TooLate
        );
        assert_eq!(
            classify_cancel(JobState::CancelPending, CancelBoundary::SafeCheckpoint),
            CancelDisposition::AlreadyRequested
        );
        assert_eq!(
            classify_cancel(JobState::Completed, CancelBoundary::SafeCheckpoint),
            CancelDisposition::AlreadyTerminal
        );
    }

    #[test]
    fn legacy_to_typed_mapping_is_total_and_marks_loss() {
        let expected = [
            (LegacyStatus::Ok, JobError::None, false),
            (
                LegacyStatus::InvalidMessage,
                JobError::InvalidMessage,
                false,
            ),
            (
                LegacyStatus::InvalidArgument,
                JobError::InvalidArgument,
                false,
            ),
            (LegacyStatus::NotFound, JobError::NotFound, false),
            (LegacyStatus::Busy, JobError::LegacyBusy, true),
            (LegacyStatus::TooLarge, JobError::ResourceExhausted, true),
            (
                LegacyStatus::StorageError,
                JobError::LegacyStorageError,
                true,
            ),
            (LegacyStatus::InvalidState, JobError::Internal, true),
            (LegacyStatus::Unsupported, JobError::Unsupported, false),
            (
                LegacyStatus::PreconditionFailed,
                JobError::PreconditionFailed,
                false,
            ),
        ];
        for (raw, (status, error, lossy)) in expected.into_iter().enumerate() {
            assert_eq!(LegacyStatus::decode(raw as u8).unwrap(), status);
            assert_eq!(error_from_legacy(status), TypedProjection { error, lossy });
        }
        assert_eq!(
            LegacyStatus::decode(10),
            Err(CodecError::UnknownLegacyStatus)
        );
    }

    #[test]
    fn typed_to_legacy_mapping_covers_every_v1_error() {
        let errors = [
            JobError::None,
            JobError::InvalidMessage,
            JobError::InvalidArgument,
            JobError::Unsupported,
            JobError::NotFound,
            JobError::BusyPlaying,
            JobError::ResourceExhausted,
            JobError::Conflict,
            JobError::PreconditionFailed,
            JobError::DeadlineExceeded,
            JobError::MediaChanged,
            JobError::StorageUnavailable,
            JobError::StorageReadFailed,
            JobError::StorageWriteFailed,
            JobError::StorageCorrupt,
            JobError::Cancelled,
            JobError::Internal,
            JobError::LegacyBusy,
            JobError::LegacyStorageError,
        ];
        for error in errors {
            let projection = error_to_legacy(error);
            assert!((projection.status as u8) <= LegacyStatus::PreconditionFailed as u8);
            if matches!(
                error,
                JobError::None
                    | JobError::InvalidMessage
                    | JobError::InvalidArgument
                    | JobError::Unsupported
                    | JobError::NotFound
                    | JobError::PreconditionFailed
            ) {
                assert!(!projection.lossy);
            } else {
                assert!(projection.lossy);
            }
        }
    }

    #[test]
    fn mixed_version_negotiation_falls_back_unless_both_edges_exist() {
        assert_eq!(negotiate_mode(0, 0), NegotiatedMode::LegacyStoppedOnly);
        assert_eq!(
            negotiate_mode(FILESYSTEM_FEATURE_PERSISTENCE_JOBS, 0),
            NegotiatedMode::LegacyStoppedOnly
        );
        assert_eq!(
            negotiate_mode(0, BRIDGE_JOB_PROTOCOL_VERSION),
            NegotiatedMode::LegacyStoppedOnly
        );
        assert_eq!(
            negotiate_mode(
                FILESYSTEM_FEATURE_PERSISTENCE_JOBS,
                BRIDGE_JOB_PROTOCOL_VERSION
            ),
            NegotiatedMode::PersistenceJobsV1
        );
    }

    #[test]
    fn caller_buffer_limits_are_explicit() {
        let request = JobRequest {
            request_id: 1,
            command: Command::Capabilities,
            client_nonce: 0,
            job_id: 0,
            total_deadline_ms: 0,
            inner_request: &[],
        };
        let mut short = [0u8; 8];
        assert_eq!(
            encode_request(request, &mut short),
            Err(CodecError::BufferTooSmall)
        );

        let mut short_capabilities = [0u8; CAPABILITIES_BODY_BYTES - 1];
        assert_eq!(
            Capabilities::V1.encode(&mut short_capabilities),
            Err(CodecError::BufferTooSmall)
        );
    }

    #[test]
    fn reserved_response_detection_never_claims_other_messages() {
        assert!(is_reserved_job_response(&[RESPONSE_MESSAGE_ID]));
        assert!(is_reserved_job_response(&[RESPONSE_MESSAGE_ID, 0xFF]));
        assert!(!is_reserved_job_response(&[]));
        assert!(!is_reserved_job_response(&[0xFB]));
    }
}
