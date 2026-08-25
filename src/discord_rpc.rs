use crate::{
    IpcPayload, VoiceSetting,
    error::{DiscordErrorCode, DiscordRPCError, EyreMutexResult, MutexError},
    get_config_path,
};
use color_eyre::{
    Section,
    eyre::{Context, Result},
};
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use reqwest::blocking::Client;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};
use std::{
    fmt::{self, Debug, Display},
    fs,
    io::{Read, Write},
    marker::PhantomData,
    os::unix::net::UnixStream,
    path::PathBuf,
};
use uuid::Uuid;

type ClientId = &'static str;
pub const CLIENT_ID: ClientId = "1535061892706598994";
pub const CLIENT_SECRET: &'static str = "eNV2xbGe90TzAlmfkvuMpZpkpZfjj_KZ";

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventKind {
    Ready,
    Error,
}

#[derive(Debug, Clone, Copy, FromPrimitive, PartialEq, Eq)]
pub enum Op {
    HandShake = 0,
    Frame = 1,
    Close = 2,
    Ping = 3,
    Pong = 4,
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format!("{:#?}", self))
    }
}

impl Op {
    #[inline]
    pub fn code(&self) -> u32 {
        *self as u32
    }
}

pub struct Payload<T> {
    op: Op,
    body: T,
}

impl<T: Serialize> Payload<T> {
    pub fn new(op: Op, body: T) -> Self {
        Self { op, body }
    }

    pub fn send<W>(&self, writer: &mut W) -> Result<()>
    where
        W: Write,
    {
        let json = serde_json::to_vec(&self.body)?;
        (|| {
            writer.write_all(&self.op.code().to_le_bytes())?;
            writer.write_all(&(json.len() as u32).to_le_bytes())?;
            writer.write_all(&json)?;
            writer.flush()?;
            Ok::<(), color_eyre::eyre::Report>(())
        })()
        .with_context(|| self.op)?;

        Ok(())
    }
}

impl<T: DeserializeOwned> Payload<T> {
    pub fn read<S>(reader: &mut S) -> EyreMutexResult<Self>
    where
        S: Read,
    {
        let mut header = [0u8; 8];
        reader.read_exact(&mut header).map_err(|e| Err(e)?)?;
        let op_code = u32::from_le_bytes(header[0..4].try_into().map_err(|e| Err(e)?)?);
        let op = Op::from_u32(op_code)
            .ok_or(Err(DiscordRPCError::UnknownOpCode).with_context(|| op_code))?;

        let body_size = u32::from_le_bytes(header[4..8].try_into().map_err(|e| Err(e)?)?);
        let mut body_data = vec![0; body_size as usize];
        reader.read_exact(&mut body_data).map_err(|e| Err(e)?)?;

        // Check if payload is error
        let error_payload = serde_json::from_slice::<PayloadEventError>(&body_data);
        if let Ok(body) = error_payload {
            if body.0.event.assert_is(EventKind::Error).is_ok() {
                println!("Error: Received discord error event:\n{:?}", body.0.data);
                return Err(Ok(
                    DiscordRPCError::UnknownErrorEvent(body.0.data.code).into()
                ));
            }
        }

        let body = serde_json::from_slice(&body_data).map_err(|e| {
            Err(e)
                .with_note(|| op)
                .with_context(|| String::from_utf8(body_data.clone()).unwrap())
        })?;

        Ok(Self { op, body })
    }
}

#[derive(Serialize)]
pub struct PayloadBodyHandShake {
    #[serde(rename = "v")]
    pub version: u32,
    pub client_id: String,
}

impl PayloadBodyHandShake {
    const HAND_SHAKE_VERSION: u32 = 1;
}

impl From<ClientId> for PayloadBodyHandShake {
    fn from(client_id: ClientId) -> Self {
        Self {
            version: Self::HAND_SHAKE_VERSION,
            client_id: client_id.to_owned(),
        }
    }
}

pub struct HandShake(Payload<PayloadBodyHandShake>);
impl HandShake {
    pub fn new(client_id: ClientId) -> Self {
        Self(Payload {
            op: Op::HandShake,
            body: PayloadBodyHandShake::from(client_id),
        })
    }

    pub fn send<S>(&self, stream: &mut S) -> EyreMutexResult<DataReady>
    where
        S: Read + Write,
    {
        self.0.send(stream).map_err(Err)?;
        Ok(EventReady::read(stream)?)
    }
}

pub struct Close;
impl Close {
    pub fn send<W: Write>(writer: &mut W) -> Result<()> {
        Payload {
            op: Op::Close,
            body: (),
        }
        .send(writer)
    }
}

pub struct FrameCommand<Args: Serialize>(Payload<PayloadFrameCommand<Args>>);
pub type FrameEvent<Data> = Payload<PayloadFrameEvent<Data>>;

impl<Args: Serialize> FrameCommand<Args> {
    pub fn new(command: Command, args: Args) -> Self {
        Self(Payload::new(
            Op::Frame,
            PayloadFrameCommand::new(command, args),
        ))
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Command {
    Authorize,
    Authenticate,
    Dispatch,
    GetVoiceSettings,
    SetVoiceSettings,
    SelectVoiceChannel,
    GetSelectVoiceChannel,
}

impl Display for EventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format!("{:?}", self))
    }
}

#[derive(Serialize)]
pub struct PayloadFrameCommand<Args: Serialize> {
    #[serde(rename = "cmd")]
    command: Command,
    args: Args,
    nonce: Uuid,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PayloadEventError(PayloadFrameEvent<EventErrorData>);

#[derive(Debug, Serialize, Deserialize)]
pub struct EventErrorData {
    code: i32,
    message: Box<str>,
}

impl Display for PayloadEventError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.data.message)?;
        f.write_str(", code: ")?;
        f.write_str(&self.0.data.code.to_string())
    }
}

impl<Args: Serialize> PayloadFrameCommand<Args> {
    pub fn new(command: Command, args: Args) -> Self {
        Self {
            command,
            args,
            nonce: Uuid::new_v4(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PayloadFrameEvent<Data> {
    #[serde(rename = "cmd")]
    command: Command,
    #[serde(rename = "evt")]
    event: EventKind,
    data: Data,
}

#[derive(Deserialize)]
pub struct PayloadFrameResponse<Data> {
    data: Data,
}

pub struct Response<Data>(Payload<PayloadFrameResponse<Data>>);
impl<Data: DeserializeOwned> Response<Data> {
    pub fn read<R: Read>(reader: &mut R) -> EyreMutexResult<Self> {
        Ok(Self(Payload::read(reader)?))
    }
}

pub trait Event: Sized {
    type Data: DeserializeOwned;
    const EVENT_KIND: EventKind;

    fn read<R>(reader: &mut R) -> EyreMutexResult<Self::Data>
    where
        R: Read,
    {
        let event = FrameEvent::read(reader)?;
        event
            .body
            .event
            .assert_is(Self::EVENT_KIND)
            .map_err(|e| Ok(e.into()))?;
        Ok(event.body.data)
    }
}

#[derive(Deserialize)]
pub struct DataReady {}
pub struct EventReady;
impl Event for EventReady {
    type Data = DataReady;
    const EVENT_KIND: EventKind = EventKind::Ready;
}

impl EventKind {
    pub fn assert_is(&self, event: EventKind) -> Result<(), DiscordRPCError> {
        if *self != event {
            Err(DiscordRPCError::WrongEvent(event, *self))?;
        }
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Rpc,
    Identify,
}

pub trait Request {
    type Args: Serialize;
    const COMMAND: Command;
    type ResponseData: DeserializeOwned;

    fn send_with_res<S: Write + Read>(
        stream: &mut S,
        args: impl Into<Self::Args>,
    ) -> EyreMutexResult<Self::ResponseData> {
        FrameCommand::new(Self::COMMAND, args.into()).send_with_res(stream)
    }

    fn send_with_before_res<S, F>(
        stream: &mut S,
        args: impl Into<Self::Args>,
        before_res: &mut F,
    ) -> EyreMutexResult<Self::ResponseData>
    where
        S: Write + Read,
        F: FnMut() -> EyreMutexResult<()>,
    {
        let cmd = FrameCommand::new(Self::COMMAND, args.into());
        cmd.send(stream).map_err(Err)?;

        before_res()?;

        Ok(cmd.wait_res(stream)?.0.body.data)
    }
}

pub struct RequestAuthorize<'a>(PhantomData<RequestAuthorizeArgs<'a>>);
impl<'a> Request for RequestAuthorize<'a> {
    type Args = RequestAuthorizeArgs<'a>;
    const COMMAND: Command = Command::Authorize;
    type ResponseData = AuthorizeResponseData;
}

#[derive(Serialize)]
pub struct RequestAuthorizeArgs<'a> {
    pub client_id: &'a str,
    pub scopes: &'a [Scope],
}

impl<Args: Serialize> FrameCommand<Args> {
    pub fn send<S: Write>(&self, writer: &mut S) -> Result<()> {
        self.0.send(writer)
    }

    pub fn wait_res<R: Read, ResponseData: DeserializeOwned>(
        &self,
        reader: &mut R,
    ) -> EyreMutexResult<Response<ResponseData>> {
        Ok(Response::read(reader)?)
    }

    pub fn send_with_res<S: Write + Read, ResponseData: DeserializeOwned>(
        &self,
        stream: &mut S,
    ) -> EyreMutexResult<ResponseData> {
        self.0.send(stream).map_err(Err)?;
        Ok(self.wait_res(stream)?.0.body.data)
    }
}

#[derive(Deserialize, Debug)]
pub struct AuthorizeResponseData {
    code: AuthCode,
}

type AuthCode = Box<str>;

#[derive(Serialize, Deserialize, Debug)]
#[serde(transparent)]
pub struct Token(pub Box<str>);
impl Token {
    fn get_save_location() -> std::io::Result<PathBuf> {
        let parent_path = get_config_path()?;
        let path = parent_path.join("token");
        Ok(path)
    }

    pub fn retrive(
        ipc_stream: &mut UnixStream,
        discord_stream: &mut UnixStream,
    ) -> EyreMutexResult<Self> {
        fn auth(
            ipc_stream: &mut UnixStream,
            discord_stream: &mut UnixStream,
        ) -> EyreMutexResult<AuthCode> {
            let res = RequestAuthorize::send_with_before_res(
                discord_stream,
                RequestAuthorizeArgs {
                    client_id: CLIENT_ID,
                    scopes: &vec![Scope::Rpc],
                },
                &mut || crate::Response::NeedsToken.send(ipc_stream).map_err(Err),
            )?;
            Ok(res.code)
        }

        fn request_token(code: &AuthCode) -> Result<Token> {
            #[derive(Deserialize)]
            struct ResBody {
                access_token: Token,
            }

            let client = Client::new();
            let res = client
                .post("https://discord.com/api/oauth2/token")
                .basic_auth(CLIENT_ID, Some(CLIENT_SECRET))
                .form(&[("grant_type", "authorization_code"), ("code", code)])
                .send()?;
            let status = res.status();
            let text = res.text().with_context(|| status)?;
            let res_body = serde_json::from_str::<ResBody>(&text).with_context(|| text)?;
            res_body.access_token.save()?;
            Ok(res_body.access_token)
        }

        let code = auth(ipc_stream, discord_stream)?;
        request_token(&code).map_err(Err)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::get_save_location()?;
        fs::write(path, &*self.0)?;
        Ok(())
    }

    pub fn read() -> std::io::Result<Self> {
        let s = fs::read_to_string(Self::get_save_location()?)?;
        Ok(Self(s.into()))
    }

    pub fn read_or_else_retrieve(
        ipc_stream: &mut UnixStream,
        discord_stream: &mut UnixStream,
    ) -> EyreMutexResult<Self> {
        Self::read().or_else(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Ok(Self::retrive(ipc_stream, discord_stream)?),
            _ => Err(Err(e.into())),
        })
    }
}

pub struct RequestAuthenticate<'a>(PhantomData<RequestAuthenticateArgs<'a>>);
impl<'a> RequestAuthenticate<'a> {
    pub fn access_token_validation_failed(res: &EyreMutexResult<RequestAuthenticateData>) -> bool {
        if let Err(e) = res {
            if let Ok(mutex_error) = e {
                if let MutexError::DiscordRPCError(rpc_error) = mutex_error {
                    if let Some(code) = rpc_error.error_event_code()
                        && code == DiscordErrorCode::InvalidAccessToken
                    {
                        return true;
                    }
                }
            }
        }
        false
    }
}

#[derive(Serialize)]
pub struct RequestAuthenticateArgs<'a> {
    access_token: &'a Token,
}

impl<'a> From<&'a Token> for RequestAuthenticateArgs<'a> {
    fn from(access_token: &'a Token) -> Self {
        Self { access_token }
    }
}

#[derive(Deserialize)]
pub struct RequestAuthenticateData {}

impl<'a> Request for RequestAuthenticate<'a> {
    type Args = RequestAuthenticateArgs<'a>;
    type ResponseData = RequestAuthenticateData;
    const COMMAND: Command = Command::Authenticate;
}

pub struct RequestSetVoiceSettings;
impl Request for RequestSetVoiceSettings {
    const COMMAND: Command = Command::SetVoiceSettings;
    type Args = RequestVoiceSettingsData;
    type ResponseData = RequestVoiceSettingsData;
}

pub struct RequestGetVoiceSettings;
impl Request for RequestGetVoiceSettings {
    const COMMAND: Command = Command::GetVoiceSettings;
    type Args = ();
    type ResponseData = RequestVoiceSettingsData;
}

#[derive(Serialize, Deserialize)]
pub struct RequestVoiceSettingsData {
    pub mode: VoiceSettingsMode,
    pub automatic_gain_control: bool,
    pub echo_cancellation: bool,
    pub noise_suppression: bool,
    pub qos: bool,
    pub silence_warning: bool,
    pub deaf: bool,
    pub mute: bool,
}

impl RequestVoiceSettingsData {
    pub fn get_setting(&mut self, setting: VoiceSetting) -> &mut bool {
        match setting {
            VoiceSetting::PushToTalk => &mut self.mode.push_to_talk,
            VoiceSetting::AutomaticGainControl => &mut self.automatic_gain_control,
            VoiceSetting::EchoCancellation => &mut self.echo_cancellation,
            VoiceSetting::NoiseSuppression => &mut self.noise_suppression,
            VoiceSetting::Qos => &mut self.qos,
            VoiceSetting::SilenceWarning => &mut self.silence_warning,
            VoiceSetting::Deaf => &mut self.deaf,
            VoiceSetting::Mute => &mut self.mute,
        }
    }
}

pub struct Ping;
impl Ping {
    pub fn send_with_res<S>(stream: &mut S) -> EyreMutexResult<()>
    where
        S: Read + Write,
    {
        Payload::new(Op::Ping, ()).send(stream).map_err(Err)?;
        let pong: Payload<()> = Payload::read(stream)?;
        if pong.op != Op::Pong {
            Err(Ok(MutexError::DiscordRPCError(
                DiscordRPCError::UnknownOpCode,
            )))?;
        }

        Ok(())
    }
}

pub type VoiceChannelId = Box<str>;

pub struct RequestSelectVoiceChannel<'a>(PhantomData<RequestSelectVoiceChannelArgs<'a>>);

#[derive(Serialize, Default)]
pub struct RequestSelectVoiceChannelArgs<'a> {
    pub channel_id: Option<&'a VoiceChannelId>,
    pub timeout: i32,
    pub force: bool,
    pub navigate: bool,
}

#[derive(Deserialize)]
pub struct RequestSelectVoiceChannelData {
    pub channel_id: VoiceChannelId,
}

impl<'a> Request for RequestSelectVoiceChannel<'a> {
    const COMMAND: Command = Command::SelectVoiceChannel;
    type Args = RequestSelectVoiceChannelArgs<'a>;
    type ResponseData = Option<RequestSelectVoiceChannelData>;
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VoiceActivationModeType {
    PushToTalk,
    VoiceActivity,
}

impl VoiceActivationModeType {
    pub fn from_push_to_talk_enabled(push_to_talk: bool) -> VoiceActivationModeType {
        if push_to_talk {
            Self::PushToTalk
        } else {
            Self::VoiceActivity
        }
    }

    pub fn into_push_to_talk_enabled(self) -> bool {
        match self {
            Self::PushToTalk => true,
            Self::VoiceActivity => false,
        }
    }
}

fn serialize_voice_activation_mode<S>(push_to_talk: &bool, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    VoiceActivationModeType::from_push_to_talk_enabled(*push_to_talk).serialize(serializer)
}

fn deserialize_voice_activation_mode<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(VoiceActivationModeType::deserialize(deserializer)?.into_push_to_talk_enabled())
}

#[derive(Serialize, Deserialize)]
pub struct VoiceSettingsMode {
    #[serde(rename = "type")]
    #[serde(serialize_with = "serialize_voice_activation_mode")]
    #[serde(deserialize_with = "deserialize_voice_activation_mode")]
    push_to_talk: bool,
}
