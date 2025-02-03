// src/core/status.rs
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub enum MessageType {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct StatusMessage {
    pub text: String,
    pub message_type: MessageType,
    pub created_at: Instant,
    pub duration: Duration,
}

impl StatusMessage {
    pub fn new(text: impl Into<String>, message_type: MessageType) -> Self {
        Self {
            text: text.into(),
            message_type,
            created_at: Instant::now(),
            duration: Duration::from_secs(3), // Default 3 second display
        }
    }

    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() >= self.duration
    }
}

#[derive(Clone, Debug)]
pub struct StatusManager {
    current_message: Option<StatusMessage>,
}

impl StatusManager {
    pub fn new() -> Self {
        Self {
            current_message: None,
        }
    }

    pub fn set_message(&mut self, message: StatusMessage) {
        self.current_message = Some(message);
    }

    pub fn info<S: Into<String>>(&mut self, text: S) {
        self.set_message(StatusMessage::new(text, MessageType::Info));
    }

    pub fn success<S: Into<String>>(&mut self, text: S) {
        self.set_message(StatusMessage::new(text, MessageType::Success));
    }

    pub fn warning<S: Into<String>>(&mut self, text: S) {
        self.set_message(StatusMessage::new(text, MessageType::Warning));
    }

    pub fn error<S: Into<String>>(&mut self, text: S) {
        self.set_message(StatusMessage::new(text, MessageType::Error));
    }

    pub fn clear(&mut self) {
        self.current_message = None;
    }

    pub fn update(&mut self) {
        if let Some(message) = &self.current_message {
            if message.is_expired() {
                self.current_message = None;
            }
        }
    }

    pub fn get_message(&self) -> Option<&StatusMessage> {
        self.current_message.as_ref()
    }
}
