//! Where a running job writes; the job service turns these into lines with timestamps.

use std::sync::Arc;

use crate::models::jobs::LineStream;
use crate::shell::Stream;

#[derive(Clone)]
pub struct Log(Arc<dyn Fn(LineStream, String) + Send + Sync>);

impl Log {
    pub fn new(append: impl Fn(LineStream, String) + Send + Sync + 'static) -> Self {
        Self(Arc::new(append))
    }

    /// Bacre narrating what it is about to do
    pub fn info(&self, text: impl Into<String>) {
        (self.0)(LineStream::Info, text.into());
    }

    /// What a tool or hook printed on stdout
    pub fn out(&self, text: impl Into<String>) {
        (self.0)(LineStream::Out, text.into());
    }

    /// What a tool or hook printed on stderr
    pub fn err(&self, text: impl Into<String>) {
        (self.0)(LineStream::Err, text.into());
    }

    /// What a shell command prints, line by line, into the log
    pub fn line(&self, stream: Stream, text: &str) {
        match stream {
            Stream::Out => self.out(text),
            Stream::Err => self.err(text),
        }
    }
}
