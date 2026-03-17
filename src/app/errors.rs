use anyhow::Error;
use std::fmt;

#[derive(Debug)]
pub struct AppError(Error);

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn into_inner(self) -> Error {
        self.0
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

impl From<Error> for AppError {
    fn from(value: Error) -> Self {
        Self(value)
    }
}
