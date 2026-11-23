use nvenc::sys::result::NVencError as RawNVEncError;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Device does not support H.264 encoding")]
    UnsupportedCodec,

    #[error("Windows error: {0}")]
    Windows(#[from] windows::core::Error),

    #[error("NVENC error: {0:?}")]
    NVEnc(#[from] NVEncError),

    #[error("The other end of an MPSC channel disconnected")]
    ChannelDisconnected,

    #[error(transparent)]
    DxgiDuplication(#[from] windows_capture::dxgi_duplication_api::Error),

    #[error(transparent)]
    Monitor(#[from] windows_capture::monitor::Error),
}

impl From<::nvenc::sys::result::NVencError> for Error {
    fn from(value: ::nvenc::sys::result::NVencError) -> Self {
        Self::NVEnc(value.into())
    }
}

#[derive(thiserror::Error, Debug)]
pub enum NVEncError {
    #[error("No device")]
    NoDevice,
    #[error("Unsupported device")]
    UnsupportedDevice,
    #[error("Invalid encoder device")]
    InvalidEncoderDevice,
    #[error("Invalid device")]
    InvalidDevice,
    #[error("Device does not exist")]
    DeviceNotExist,
    #[error("Invalid pointer")]
    InvalidPtr,
    #[error("Invalid event")]
    InvalidEvent,
    #[error("Invalid parameter")]
    InvalidParam,
    #[error("Invalid call")]
    InvalidCall,
    #[error("Out-of-memory")]
    OutOfMemory,
    #[error("Not initialized")]
    NotInitialized,
    #[error("Unsupported parameter")]
    UnsupportedParam,
    #[error("Buffer lock is busy")]
    LockBusy,
    #[error("Not enough buffer")]
    NotEnoughBuffer,
    #[error("Invalid version")]
    InvalidVersion,
    #[error("Memory map failed")]
    MapFailed,
    #[error("Need more input")]
    NeedMoreInput,
    #[error("Encoder is busy")]
    EncoderBusy,
    #[error("Event not registered")]
    EventNotRegisterd,
    #[error("Generic error")]
    Generic,
    #[error("Incompatible client key")]
    IncompatibleClientKey,
    #[error("Unimplemented")]
    Unimplemented,
    #[error("Resource registration failed")]
    ResourceRegisterFailed,
    #[error("Resource not registered")]
    ResourceNotRegistered,
    #[error("Resource not mapped")]
    ResourceNotMapped,
    #[error("Need more output")]
    NeedMoreOutput,
}

impl From<RawNVEncError> for NVEncError {
    fn from(raw: RawNVEncError) -> Self {
        macro_rules! map {
            (match ($raw:expr) { $($variant:ident,)* }) => {
                match $raw {
                    $(RawNVEncError::$variant => Self::$variant,)*
                }
            }
        }
        map!(match (raw) {
            NoDevice,
            UnsupportedDevice,
            InvalidEncoderDevice,
            InvalidDevice,
            DeviceNotExist,
            InvalidPtr,
            InvalidEvent,
            InvalidParam,
            InvalidCall,
            OutOfMemory,
            NotInitialized,
            UnsupportedParam,
            LockBusy,
            NotEnoughBuffer,
            InvalidVersion,
            MapFailed,
            NeedMoreInput,
            EncoderBusy,
            EventNotRegisterd,
            Generic,
            IncompatibleClientKey,
            Unimplemented,
            ResourceRegisterFailed,
            ResourceNotRegistered,
            ResourceNotMapped,
            NeedMoreOutput,
        })
    }
}
