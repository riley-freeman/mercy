use crate::context::Context;

/// A handle to a worker within our family
#[derive(Debug, Clone, Copy)]
pub struct Worker {
    id: u64,
    context: Context,
}

impl Worker {
    pub fn new(context: Context, id: u64) -> Self {
        Self { id, context }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn send_message(
        &self,
        message: impl serde::Serialize,
        callback: impl FnOnce(serde_value::Value) + Send + 'static,
    ) -> Result<(), crate::error::Error> {
        self.context.send_message(self, message, callback)
    }
}
