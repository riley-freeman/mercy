use std::os::unix::net::UnixStream;

pub struct WorkerInfo {
    pub(crate) stream: UnixStream,
    pub state: Option<u128>,
}

impl WorkerInfo {
    pub fn new(stream: UnixStream) -> Self {
        Self {
            stream,
            state: None,
        }
    }

    pub fn stream(&self) -> &UnixStream {
        &self.stream
    }
}
