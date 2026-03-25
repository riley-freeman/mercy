use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum MessageType {
    Alloc,
    Free,
    MapId,
    Exit,
    Shutdown,
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
