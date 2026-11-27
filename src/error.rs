use nvenc::sys::result::{AsyncNVencError, NVencError};

use std::sync::mpsc::{RecvError, SendError};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Device does not support H.264 encoding")]
    UnsupportedCodec,

    #[error("Windows error: {0}")]
    Windows(#[from] windows::core::Error),

    #[error("Win32 error: {0:?}")]
    Win32(windows::Win32::Foundation::WIN32_ERROR),

    #[error("NVENC error: {0:?}")]
    NVEnc(#[from] NVencError),

    #[error("The other end of an MPSC channel disconnected")]
    ChannelDisconnected,

    #[error(transparent)]
    DxgiDuplication(#[from] windows_capture::dxgi_duplication_api::Error),

    #[error(transparent)]
    Monitor(#[from] windows_capture::monitor::Error),
}

impl From<AsyncNVencError> for Error {
    fn from(value: AsyncNVencError) -> Self {
        match value {
            AsyncNVencError::NVenc(err) => Self::NVEnc(err),
            AsyncNVencError::Windows(err) => Self::Windows(err),
            AsyncNVencError::Win32(err) => Self::Win32(err),
        }
    }
}

impl<T> From<SendError<T>> for Error {
    fn from(_value: SendError<T>) -> Self {
        Self::ChannelDisconnected
    }
}

impl From<RecvError> for Error {
    fn from(_value: RecvError) -> Self {
        Self::ChannelDisconnected
    }
}
