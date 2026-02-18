#[derive(Debug)]
pub enum Error {
    InvalidSize { size: usize },
    InvalidPermissions { id: String },

    IdAlreadyExists { id: String },
    RequestedAllocInfoNotFound { id: u128 },
    RequestedAllocatorNotFound { id: u128 },
    RequestedContextNotFound { id: u128 },

    JobNameReserved { name: String },

    OperationUnsupported,

    ProcessLimitReached,

    NoBlocksAvailable { requested: usize },
    BlockNotFound { allocation_id: u128 },

    MiscellaneousOSError { code: i32 },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidSize { size } => write!(f, "Invalid size: {}", size),
            Error::InvalidPermissions { id } => write!(f, "Invalid permissions for id: {}", id),

            Error::IdAlreadyExists { id } => write!(f, "ID already exists: {}", id),
            Error::RequestedAllocInfoNotFound { id } => {
                write!(f, "ID does not contain the proper information: {}", id)
            }
            Error::RequestedAllocatorNotFound { id } => {
                write!(f, "ID does not contain a findable allocator: {}", id)
            }
            Error::RequestedContextNotFound { id } => {
                write!(f, "ID does not contain a registered context: {}", id)
            }

            Error::JobNameReserved { name } => write!(f, "Job name {} is reserved", name),

            Error::OperationUnsupported => write!(f, "Operation not supported on this machine"),

            Error::ProcessLimitReached => {
                write!(f, "Process has reached its limit of shared memory segments")
            }

            Error::NoBlocksAvailable { requested } => {
                write!(f, "No blocks available for allocation: {}", requested)
            }
            Error::BlockNotFound { allocation_id } => write!(
                f,
                "Block from context: {} and ID: {} could not be found",
                (allocation_id >> 64) as u64,
                (allocation_id >> 16) as u16
            ),

            Error::MiscellaneousOSError { code } => write!(f, "Miscellaneous OS error: {}", code),
        }
    }
}
