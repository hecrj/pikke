use std::num;

#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    #[error("an integer was out of range: {0}")]
    IntegerOutOfRange(num::TryFromIntError),
    #[error("invalid dimensions {width}x{height}")]
    InvalidDimensions { width: u16, height: u16 },
}

impl From<num::TryFromIntError> for Error {
    fn from(error: num::TryFromIntError) -> Self {
        Self::IntegerOutOfRange(error)
    }
}

impl From<Error> for mlua::Error {
    fn from(error: Error) -> Self {
        Self::RuntimeError(error.to_string())
    }
}
