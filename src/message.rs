use libc::{__SIZEOF_PTHREAD_MUTEX_T, pthread_mutex_t};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum MessageType {
    Alloc,
    Free,
    MapId,
    Exit,
    Shutdown,

    NewWorker,
    SendWorker,
    ResponseWorker,

    NewMutex,
    GetPlatformMutex,

    SetWorkerState,
    GetWorkerState,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Message<T> {
    pub id: i64,
    pub reply_id: Option<i64>,
    pub message_type: MessageType,
    pub message_data: T,
}

impl<T> Message<T> {
    pub fn new(id: i64, message_type: MessageType, data: T) -> Self {
        Self {
            id,
            reply_id: None,
            message_type,
            message_data: data,
        }
    }

    pub fn with_reply(id: i64, reply_id: i64, message_type: MessageType, data: T) -> Self {
        Self {
            id,
            reply_id: Some(reply_id),
            message_type,
            message_data: data,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AllocData {
    pub family_id: i64,
    pub size: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AllocReply {
    pub alloc_id_high: u64,
    pub alloc_id_low: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FreeData {
    pub alloc_id_high: u64,
    pub alloc_id_low: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MapIdData {
    pub alloc_id_high: u64,
    pub alloc_id_low: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NewWorkerData {
    pub worker_role: String,
    pub arguments: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NewWorkerReply {
    pub worker_id: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SendWorkerMessage {
    pub worker_id: u64,
    pub message_data: serde_value::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SendWorkerReply {
    pub worker_id: u64,
    pub message_data: serde_value::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RecvWorkerMessage<T> {
    pub worker_id: u64,
    pub message_data: T,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NewMutexData {}

#[cfg(target_family = "unix")]
#[derive(Debug, Serialize, Deserialize)]
pub struct NewMutexReply {
    pub pthread_mutex: Vec<u8>,
    pub mutex_id: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetPlatformMutex {
    pub mutex_id: u64,
}

#[cfg(target_family = "unix")]
#[derive(Debug, Serialize, Deserialize)]
pub struct GetPlatformMutexReply {
    pub pthread_mutex: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetWorkerStateData {
    pub state_id_high: u64,
    pub state_id_low: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetWorkerStateData {
    pub worker_id: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetWorkerStateReply {
    pub state_id_high: Option<u64>,
    pub state_id_low: Option<u64>,
}
