pub mod discord_rpc;
pub mod error;

use crate::error::MutexError;
use clap::ValueEnum;
use color_eyre::eyre::Result;
use core::slice;
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
};

#[derive(Clone, Copy, Debug)]
pub enum Action {
    Force(bool),
    Toggle,
}

#[derive(Clone, Copy, ValueEnum, Debug, PartialEq, Eq)]
pub enum VoiceSetting {
    AutomaticGainControl,
    EchoCancellation,
    NoiseSuppression,
    Qos,
    SilenceWarning,
    Deaf,
    Mute,
}

#[derive(Debug, Clone, Copy)]
pub struct Changer {
    pub action: Action,
    pub setting: VoiceSetting,
}

#[derive(Debug)]
#[repr(C)]
pub struct Request {
    pub changer: Changer,
}

#[repr(C)]
pub enum Response {
    Done,
    NeedsToken,
    Error(MutexError),
}

pub fn get_config_path() -> std::io::Result<PathBuf> {
    let parent_path = dirs::data_local_dir().unwrap().join("discord-mutex");
    if !parent_path.exists() {
        fs::create_dir_all(&parent_path)?;
    }

    Ok(parent_path)
}

pub fn get_ipc_path() -> Result<PathBuf> {
    Ok(get_config_path()?.join("ipc.sock"))
}

pub trait IpcPayload: Sized {
    fn send<W: Write>(&self, writer: &mut W) -> Result<()> {
        let buf = unsafe {
            slice::from_raw_parts(
                (self as *const Self) as *const u8,
                std::mem::size_of::<Self>(),
            )
        };

        writer.write_all(buf)?;
        writer.flush()?;

        Ok(())
    }

    fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let mut buf = vec![0u8; std::mem::size_of::<Self>()];

        reader.read_exact(&mut buf)?;

        Ok(unsafe { std::ptr::read_unaligned(buf.as_ptr() as *const Self) })
    }
}

impl IpcPayload for Request {}
impl IpcPayload for Response {}
