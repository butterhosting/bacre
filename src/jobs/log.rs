use std::sync::Arc;

use crate::models::jobs::LineStream;
use crate::shell::Stream;

#[derive(Clone)]
pub struct Log(Arc<dyn Fn(LineStream, String) + Send + Sync>);

impl Log {
    pub fn new(append: impl Fn(LineStream, String) + Send + Sync + 'static) -> Self {
        Self(Arc::new(append))
    }

    pub fn info(&self, text: impl Into<String>) {
        (self.0)(LineStream::Info, text.into());
    }

    pub fn out(&self, text: impl Into<String>) {
        (self.0)(LineStream::Out, text.into());
    }

    pub fn err(&self, text: impl Into<String>) {
        (self.0)(LineStream::Err, text.into());
    }

    pub fn line(&self, stream: Stream, text: &str) {
        match stream {
            Stream::Out => self.out(text),
            Stream::Err => self.err(text),
        }
    }
}
