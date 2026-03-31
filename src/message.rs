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
