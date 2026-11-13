use std::io;
use std::num;
use std::path::PathBuf;
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
    #[error("{0}")]
    Lua(Arc<mlua::Error>),
    #[error("{option} requires a value")]
    OptionMissingValue { option: String },
    #[error("invalid {option} value: {value} ({note})")]
    OptionInvalidValue {
        option: String,
        value: String,
        note: String,
    },
    #[error("invalid option: {option}")]
    OptionUnknown { option: String },
    #[error("invalid argument: {argument}")]
    ArgumentUnknown { argument: String },
    #[error("failed to read {script}: {error}")]
    ReadFailed {
        script: PathBuf,
        error: Arc<io::Error>,
    },
    #[error("failed to load {path}: {error}")]
    FontReadFailed {
        path: PathBuf,
        error: Arc<io::Error>,
    },
    #[error("{path} is not a valid font")]
    FontInvalid { path: PathBuf },
    #[error("script {script}: the module must be a table ({error})")]
    ModuleNotTable {
        script: PathBuf,
        error: Arc<mlua::Error>,
    },
    #[error("script {script}: 'render' is missing or not a function")]
    RenderMissing { script: PathBuf },
    #[error("script {script}: 'arguments' must be a table ({error})")]
    ArgumentsNotTable {
        script: PathBuf,
        error: Arc<mlua::Error>,
    },
    #[error("script {script}: '{name}' is a reserved argument name")]
    ArgumentReserved { script: PathBuf, name: String },
    #[error("script {script}: 'arguments' entry {index} must be a string ({error})")]
    ArgumentNotString {
        script: PathBuf,
        index: usize,
        error: Arc<mlua::Error>,
    },
    #[error("failed create file watcher: {0}")]
    WatcherFailed(Arc<notify::Error>),
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

impl From<mlua::Error> for Error {
    fn from(error: mlua::Error) -> Self {
        Self::Lua(Arc::new(error))
    }
}

impl From<Error> for mlua::Error {
    fn from(error: Error) -> Self {
        Self::RuntimeError(error.to_string())
    }
}

impl From<notify::Error> for Error {
    fn from(error: notify::Error) -> Self {
        Self::WatcherFailed(Arc::new(error))
    }
}
