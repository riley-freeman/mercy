#[derive(Debug)]
pub enum Error {
    InvalidSize {
        size: usize,
    },
    InvalidPermissions {
        id: String,
    },

    IdAlreadyExists {
        id: String,
    },
    IdNotFound {
        id: String,
    },
    RequestedAllocInfoNotFound {
        id: u128,
    },
    RequestedAllocatorNotFound {
        id: u128,
    },
    RequestedContextNotFound {
        id: u128,
    },

    RoleNameReserved {
        name: String,
    },

    OperationUnsupported,

    ProcessLimitReached,

    NoBlocksAvailable {
        requested: usize,
    },
    BlockNotFound {
        allocation_id: u128,
    },

    UnexpectedMessageType {
        message_type: String,
    },

    IoError {
        io_error: std::io::Error,
    },

    CannotStartProcess {
        io_error: std::io::Error,
    },

    #[cfg(target_os = "linux")]
    ShmemError {
        shmem_error: shared_memory::ShmemError,
    },

    MiscellaneousOSError {
        code: i32,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidSize { size } => write!(f, "Invalid size: {}", size),
            Error::InvalidPermissions { id } => write!(f, "Invalid permissions for id: {}", id),

            Error::IdAlreadyExists { id } => write!(f, "ID already exists: {}", id),
            Error::IdNotFound { id } => write!(f, "ID not found: {}", id),
            Error::RequestedAllocInfoNotFound { id } => {
                write!(f, "ID does not contain the proper information: {}", id)
            }
            Error::RequestedAllocatorNotFound { id } => {
                write!(f, "ID does not contain a findable allocator: {}", id)
            }
            Error::RequestedContextNotFound { id } => {
                write!(f, "ID does not contain a registered context: {}", id)
            }

            Error::RoleNameReserved { name } => write!(f, "Role name {} is reserved", name),

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

            Error::UnexpectedMessageType { message_type } => {
                write!(f, "Unexpected message type: {}", message_type)
            }

            Error::IoError { io_error } => write!(f, "IO error: {}", io_error),
            Error::CannotStartProcess { io_error } => {
                write!(f, "Failed to start process: {}", io_error)
            }

            #[cfg(target_os = "linux")]
            Error::ShmemError { shmem_error } => write!(f, "Shmem error: {}", shmem_error),

            Error::MiscellaneousOSError { code } => write!(f, "Miscellaneous OS error: {}", code),
        }
    }
}
