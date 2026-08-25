use color_eyre::Result;
use discord_mutex::{
    Action, Changer, IpcPayload, Request as IpcRequest, Response as IpcResponse, VoiceSetting,
    discord_rpc::{
        CLIENT_ID, Close, HandShake, Request as RpcRequest, RequestAuthenticate,
        RequestGetVoiceSettings, RequestSetVoiceSettings, Token,
    },
    error::{DiscordRPCError, EyreMutexResult, MutexError},
    get_ipc_path,
};
use std::{
    fs,
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
};

fn find_ipc_dir() -> Result<PathBuf> {
    let file_name = "discord-ipc-";
    #[cfg(target_os = "linux")]
    let dir = std::env::var("XDG_RUNTIME_DIR")
        .or_else(|_| std::env::var("TMPDIR"))
        .or_else(|_| std::env::var("TMP"))
        .or_else(|_| std::env::var("TEMP"))
        .unwrap_or(String::from("/tmp"));

    for n in 0..10 {
        let path = PathBuf::from(&dir).join(file_name.to_owned() + &n.to_string());

        if fs::exists(&path)? {
            return Ok(path);
        }
    }

    Err(DiscordRPCError::IpcConnectionFailed)?
}

pub fn try_connect_discord(ipc_stream: &mut UnixStream) -> EyreMutexResult<UnixStream> {
    let ipc_path = find_ipc_dir().map_err(Err)?;
    let mut stream = UnixStream::connect(ipc_path).map_err(|e| match e.kind() {
        std::io::ErrorKind::ConnectionRefused => Ok(DiscordRPCError::IpcConnectionFailed.into()),
        _ => Err(e)?,
    })?;

    let _ready_data = HandShake::new(CLIENT_ID).send(&mut stream)?;

    let mut token = Token::read_or_else_retrieve(ipc_stream, &mut stream)?;
    // TODO: Verify token not expired
    loop {
        let res = RequestAuthenticate::send_with_res(&mut stream, &token);
        let wrong_token = RequestAuthenticate::access_token_validation_failed(&res);
        if wrong_token {
            // Try get a new one and retry auth
            token = Token::retrive(ipc_stream, &mut stream)?;
        } else {
            // Handle any other exception and move on
            res?;
            break;
        }
    }

    Ok(stream)
}

// Temp function, placeholder for {Option::get_or_try_insert_with} which is currently unstable
pub fn get_or_try_insert_with<'a, T, F, E>(
    opt: &'a mut Option<T>,
    f: &mut F,
) -> Result<&'a mut T, E>
where
    F: FnMut() -> Result<T, E>,
{
    match opt {
        Some(inner) => Ok(inner),
        None => {
            let new = f()?;
            *opt = Some(new);
            Ok(opt.as_mut().unwrap())
        }
    }
}

fn main() -> Result<()> {
    color_eyre::install()?;

    let ipc_path = get_ipc_path()?;
    if ipc_path.try_exists()? {
        fs::remove_file(&ipc_path)?;
    }

    let listener = UnixListener::bind(&ipc_path)?;
    let mut discord_stream_opt: Option<UnixStream> = None;

    for stream_res in listener.incoming() {
        let mut ipc_stream = stream_res?;
        handle_client(&mut discord_stream_opt, &mut ipc_stream)?;
    }

    // Clean up logic
    if let Some(discord_stream) = &mut discord_stream_opt {
        Close::send(discord_stream)?;
    }
    fs::remove_file(ipc_path)?;

    Ok(())
}

fn apply_changer(discord_stream: &mut UnixStream, changer: &Changer) -> EyreMutexResult<()> {
    let mut state = RequestGetVoiceSettings::send_with_res(discord_stream, ())?;
    let current_setting_value = state.get_setting(changer.setting);
    *current_setting_value = match changer.action {
        Action::Force(bool) => bool,
        Action::Toggle => !*current_setting_value,
    };

    if changer.setting == VoiceSetting::Deaf {
        // Small truth table helper
        // if mute && !deaf => mute
        // if !mute && !deaf => unmute
        // if mute && deaf => mute
        // if !mute && deaf => mute
        state.mute = state.mute || state.deaf;
    }

    RequestSetVoiceSettings::send_with_res(discord_stream, state)?;
    Ok(())
}

fn handle_client(
    discord_stream_opt: &mut Option<UnixStream>,
    ipc_stream: &mut UnixStream,
) -> Result<()> {
    let req = IpcRequest::read(ipc_stream)?;

    let error_prone = (|| {
        let discord_stream =
            get_or_try_insert_with(discord_stream_opt, &mut || try_connect_discord(ipc_stream))?;
        match &req {
            IpcRequest::Set(changer) => apply_changer(discord_stream, changer),
        }
    })();

    let res = match error_prone {
        Ok(()) => IpcResponse::Done,
        Err(err) => match err {
            Ok(e) => IpcResponse::Error(e),
            Err(report) => {
                IpcResponse::Error(MutexError::Unknown).send(ipc_stream)?;
                Err(report)?
            }
        },
    };
    res.send(ipc_stream)?;
    Ok(())
}
