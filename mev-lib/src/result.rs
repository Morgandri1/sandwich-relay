use std::fmt;

pub type MevResult<T> = Result<T, MevError>;

#[derive(Debug)]
pub enum MevError {
    ConversionWouldOverflow,
    FailedToDeserialize,
    FailedToSerialize,
    ValueError,
    FailedToBuildTx,
    UnknownError,
    IncorrectProgram,
    AccountsError
}

impl fmt::Display for MevError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConversionWouldOverflow => write!(f, "Numeric conversion would overflow"),
            Self::FailedToDeserialize => write!(f, "Failed to deserialize"),
            Self::FailedToSerialize => write!(f, "Failed to serialize"),
            Self::ValueError => write!(f, "Value Error"),
            Self::FailedToBuildTx => write!(f, "Failed to build transaction"),
            Self::UnknownError => write!(f, "an Unknown Error occured"),
            Self::IncorrectProgram => write!(f, "Passed incorrect program to deserializer"),
            Self::AccountsError => write!(f, "invalid or incorrect accounts were passed to the program")
        }
    }
}