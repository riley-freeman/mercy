#[derive(Debug)]
pub enum Error {
    InvalidSize{size: usize},
    InvalidPermissions{id: String},

    IdAlreadyExists{id: String},

    OperationUnsupported,

    ProcessLimitReached,

    NoBlocksAvailable {requested: usize},
    MiscellaneousOSError {code: i32},
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidSize{size} => write!(f, "Invalid size: {}", size),
            Error::InvalidPermissions{id} => write!(f, "Invalid permissions for id: {}", id),

            Error::IdAlreadyExists{id} => write!(f, "ID already exists: {}", id),

            Error::OperationUnsupported => write!(f, "Operation not supported on this machine"),

            Error::ProcessLimitReached => write!(f, "Process has reached its limit of shared memory segments"),

            Error::NoBlocksAvailable {requested} => write!(f, "No blocks available for allocation: {}", requested),

            Error::MiscellaneousOSError{code} => write!(f, "Miscellaneous OS error: {}", code),
        }
    }
}