use std::io;
use std::num;
use std::sync::Arc;

#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    #[error("an integer was out of range: {0}")]
    IntegerOutOfRange(#[from] num::TryFromIntError),
    #[error("invalid dimensions {width}x{height}")]
    InvalidDimensions { width: u16, height: u16 },
    #[error("the video requires even dimensions: {width}x{height}")]
    OddDimensions { width: u16, height: u16 },
    #[error("invalid pixel data {width}x{height}: expected {expected} bytes, got {got}")]
    InvalidPixelData {
        width: u16,
        height: u16,
        expected: usize,
        got: usize,
    },
    #[error("image operation failed: {0}")]
    ImageOperationFailed(Arc<image::ImageError>),
    #[error("IO operation failed: {0}")]
    IoFailed(Arc<io::Error>),
}

impl From<image::ImageError> for Error {
    fn from(error: image::ImageError) -> Self {
        Self::ImageOperationFailed(Arc::new(error))
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::IoFailed(Arc::new(error))
    }
}

impl From<Error> for mlua::Error {
    fn from(error: Error) -> Self {
        Self::RuntimeError(error.to_string())
    }
}
