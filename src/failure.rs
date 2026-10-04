//! How a job, or a step of one, fails: a message for the log and the notification, and the
//! services known to be the reason (a job can fail without knowing which: then none).

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub message: String,
    pub services: Vec<String>,
}

impl Failure {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            services: Vec::new(),
        }
    }

    pub fn of(message: impl Into<String>, services: Vec<String>) -> Self {
        Self {
            message: message.into(),
            services,
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self::new(message)
    }
}

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        Self::new(error.to_string())
    }
}

pub type Outcome<T = ()> = Result<T, Failure>;
