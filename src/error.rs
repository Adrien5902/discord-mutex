use crate::discord_rpc::EventKind;
use color_eyre::eyre::Result;
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
pub enum MutexError {
    #[error("Error with discord: \n{0}")]
    DiscordRPCError(DiscordRPCError),
    #[error(
        "Deamon wasn't started, make sure it's running, use: \"systemctl --user enable --now discord-mutexd\""
    )]
    DeamonNotStarted,
    #[error("Deamon crashed unexpectedly look at error log")]
    Unknown,
}

#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
pub enum DiscordRPCError {
    #[error("IPC connection failed, make sure discord is running")]
    IpcConnectionFailed,
    #[error("Recieve an unknown op code in a response")]
    UnknownOpCode,
    #[error("Wrong event recieved, expected {0}, got {1}")]
    WrongEvent(EventKind, EventKind),
    #[error("Received an error event from discord with code: {0}\nSee daemon logs for more info")]
    UnknownErrorEvent(i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromPrimitive)]
#[repr(i32)]
pub enum DiscordErrorCode {
    InvalidAccessToken = 4009,
}

impl DiscordRPCError {
    pub fn error_event_code(&self) -> Option<DiscordErrorCode> {
        let Self::UnknownErrorEvent(code) = self else {
            return None;
        };
        DiscordErrorCode::from_i32(*code)
    }
}

impl Into<MutexError> for DiscordRPCError {
    fn into(self) -> MutexError {
        MutexError::DiscordRPCError(self)
    }
}

pub type MutexResult<T> = Result<T, MutexError>;
pub type EyreMutexResult<T> = Result<T, Result<MutexError>>;
