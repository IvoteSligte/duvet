use nvenc::sys::result::NVencError as RawError;

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

impl From<RawError> for NVEncError {
    fn from(raw: RawError) -> Self {
        macro_rules! map {
            (match ($raw:expr) { $($variant:ident,)* }) => {
                match $raw {
                    $(RawError::$variant => Self::$variant,)*
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
