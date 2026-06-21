//! Types for developing Parties server plugins.
//!
//! The `abi` module mirrors the native plugin ABI from `parties/plugin_api.h`.
//! The remaining types are Rust-side helpers for manifests, permissions, and
//! wire constants that plugin hosts and SDKs need to agree on.

pub mod abi {
  use core::{
    ffi::{c_char, c_void},
    marker::{PhantomData, PhantomPinned},
  };

  pub const API_VERSION_MAJOR: u16 = 1;
  pub const API_VERSION_MINOR: u16 = 1;
  pub const API_VERSION: &str = "1.1";

  #[repr(C)]
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub struct AbiHeader {
    pub size: u32,
    pub api_major: u16,
    pub api_minor: u16,
  }

  impl AbiHeader {
    pub const fn new<T>() -> Self {
      Self {
        size: core::mem::size_of::<T>() as u32,
        api_major: API_VERSION_MAJOR,
        api_minor: API_VERSION_MINOR,
      }
    }

    pub const fn is_compatible_with<T>(self) -> bool {
      self.api_major == API_VERSION_MAJOR && self.size as usize >= core::mem::size_of::<T>()
    }
  }

  pub type SessionId = u32;
  pub type UserId = u32;
  pub type ChannelId = u32;
  pub type MessageId = u64;

  #[repr(C)]
  pub struct Bot {
    _private: [u8; 0],
    _pinned: PhantomData<(*mut u8, PhantomPinned)>,
  }

  pub type BotHandle = *mut Bot;
  pub const MAX_NAME_LEN: usize = 128;
  pub const MAX_FINGERPRINT_LEN: usize = 192;

  #[repr(C)]
  #[derive(Debug, Clone, Copy)]
  pub struct SessionInfo {
    pub abi: AbiHeader,
    pub session_id: SessionId,
    pub user_id: UserId,
    pub voice_channel_id: ChannelId,
    pub role: u8,
    pub authenticated: u8,
    pub muted: u8,
    pub deafened: u8,
    pub username: [c_char; MAX_NAME_LEN],
  }

  impl Default for SessionInfo {
    fn default() -> Self {
      Self {
        abi: AbiHeader::new::<Self>(),
        session_id: 0,
        user_id: 0,
        voice_channel_id: 0,
        role: 0,
        authenticated: 0,
        muted: 0,
        deafened: 0,
        username: [0; MAX_NAME_LEN],
      }
    }
  }

  #[repr(C)]
  #[derive(Debug, Clone, Copy)]
  pub struct UserInfo {
    pub abi: AbiHeader,
    pub user_id: UserId,
    pub role: u8,
    pub is_bot: u8,
    pub display_name: [c_char; MAX_NAME_LEN],
    pub fingerprint: [c_char; MAX_FINGERPRINT_LEN],
    pub bot_owner_plugin: [c_char; MAX_NAME_LEN],
    pub bot_key: [c_char; MAX_NAME_LEN],
  }

  impl Default for UserInfo {
    fn default() -> Self {
      Self {
        abi: AbiHeader::new::<Self>(),
        user_id: 0,
        role: 0,
        is_bot: 0,
        display_name: [0; MAX_NAME_LEN],
        fingerprint: [0; MAX_FINGERPRINT_LEN],
        bot_owner_plugin: [0; MAX_NAME_LEN],
        bot_key: [0; MAX_NAME_LEN],
      }
    }
  }

  #[repr(C)]
  #[derive(Debug, Clone, Copy)]
  pub struct ChannelInfo {
    pub abi: AbiHeader,
    pub channel_id: ChannelId,
    pub user_count: u32,
    pub max_users: i32,
    pub sort_order: i32,
    pub name: [c_char; MAX_NAME_LEN],
  }

  impl Default for ChannelInfo {
    fn default() -> Self {
      Self {
        abi: AbiHeader::new::<Self>(),
        channel_id: 0,
        user_count: 0,
        max_users: 0,
        sort_order: 0,
        name: [0; MAX_NAME_LEN],
      }
    }
  }

  #[repr(u8)]
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
  }

  impl LogLevel {
    pub const fn from_u8(value: u8) -> Option<Self> {
      match value {
        0 => Some(Self::Trace),
        1 => Some(Self::Debug),
        2 => Some(Self::Info),
        3 => Some(Self::Warn),
        4 => Some(Self::Error),
        _ => None,
      }
    }
  }

  #[repr(u8)]
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum CommandArgType {
    String = 0,
    Bool = 1,
    Int8 = 2,
    UInt8 = 3,
    Int16 = 4,
    UInt16 = 5,
    Int32 = 6,
    UInt32 = 7,
    Int64 = 8,
    UInt64 = 9,
    Float = 10,
    Double = 11,
  }

  impl CommandArgType {
    pub const fn from_u8(value: u8) -> Option<Self> {
      match value {
        0 => Some(Self::String),
        1 => Some(Self::Bool),
        2 => Some(Self::Int8),
        3 => Some(Self::UInt8),
        4 => Some(Self::Int16),
        5 => Some(Self::UInt16),
        6 => Some(Self::Int32),
        7 => Some(Self::UInt32),
        8 => Some(Self::Int64),
        9 => Some(Self::UInt64),
        10 => Some(Self::Float),
        11 => Some(Self::Double),
        _ => None,
      }
    }
  }

  #[repr(C)]
  #[derive(Debug, Clone, Copy)]
  pub struct CommandArgumentValue {
    pub abi: AbiHeader,
    pub name: *const c_char,
    pub type_: u8,
    pub present: u8,
    pub i64_value: i64,
    pub u64_value: u64,
    pub f64_value: f64,
    pub bool_value: u8,
    pub string_value: *const c_char,
  }

  #[repr(u8)]
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum CommandInputMode {
    None = 0,
    LiveQuery = 1,
  }

  impl CommandInputMode {
    pub const fn from_u8(value: u8) -> Option<Self> {
      match value {
        0 => Some(Self::None),
        1 => Some(Self::LiveQuery),
        _ => None,
      }
    }
  }

  #[repr(C)]
  #[derive(Debug, Clone, Copy)]
  pub struct CommandInputDefinition {
    pub abi: AbiHeader,
    pub argument_name: *const c_char,
    pub mode: u8,
    pub min_chars: u16,
    pub debounce_ms: u16,
    pub max_results: u16,
    pub placeholder: *const c_char,
  }

  #[repr(C)]
  #[derive(Debug, Clone, Copy)]
  pub struct CommandDefinition {
    pub abi: AbiHeader,
    pub name: *const c_char,
    pub description: *const c_char,
    pub usage: *const c_char,
    pub min_role: u8,
    pub inputs: *const CommandInputDefinition,
    pub input_count: usize,
  }

  #[repr(C)]
  #[derive(Debug, Clone, Copy)]
  pub struct PluginVariable {
    pub abi: AbiHeader,
    pub key: *const c_char,
    pub value: *const c_char,
  }

  #[repr(C)]
  #[derive(Debug, Clone, Copy)]
  pub struct ChatCommandInvocation {
    pub abi: AbiHeader,
    pub session_id: SessionId,
    pub user_id: UserId,
    pub text_channel_id: ChannelId,
    pub caller_role: u8,
    pub command_name: *const c_char,
    pub args: *const c_char,
    pub raw_text: *const c_char,
    pub parsed_args: *const CommandArgumentValue,
    pub parsed_arg_count: usize,
  }

  #[repr(u8)]
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum CommandQueryStatus {
    Ok = 0,
    NoResults = 1,
    TooShort = 2,
    RateLimited = 3,
    PluginError = 4,
    PermissionDenied = 5,
    Pending = 6,
  }

  impl CommandQueryStatus {
    pub const fn from_u8(value: u8) -> Option<Self> {
      match value {
        0 => Some(Self::Ok),
        1 => Some(Self::NoResults),
        2 => Some(Self::TooShort),
        3 => Some(Self::RateLimited),
        4 => Some(Self::PluginError),
        5 => Some(Self::PermissionDenied),
        6 => Some(Self::Pending),
        _ => None,
      }
    }
  }

  #[repr(C)]
  #[derive(Debug, Clone, Copy)]
  pub struct CommandQueryRequest {
    pub abi: AbiHeader,
    pub session_id: SessionId,
    pub user_id: UserId,
    pub text_channel_id: ChannelId,
    pub caller_role: u8,
    pub request_id: u64,
    pub command_name: *const c_char,
    pub argument_name: *const c_char,
    pub query: *const c_char,
    pub cursor_pos: u16,
  }

  #[repr(C)]
  #[derive(Debug, Clone, Copy)]
  pub struct CommandQueryResult {
    pub abi: AbiHeader,
    pub id: *const c_char,
    pub title: *const c_char,
    pub subtitle: *const c_char,
    pub value: *const c_char,
    pub kind: *const c_char,
    pub duration_ms: u32,
    pub thumbnail_url: *const c_char,
  }

  #[repr(C)]
  #[derive(Debug, Clone, Copy)]
  pub struct CommandQueryResponse {
    pub abi: AbiHeader,
    pub status: u8,
    pub message: *const c_char,
    pub results: *const CommandQueryResult,
    pub result_count: usize,
  }

  #[repr(C)]
  #[derive(Debug, Clone, Copy)]
  pub struct ChatMessage {
    pub abi: AbiHeader,
    pub session_id: SessionId,
    pub author_user_id: UserId,
    pub text_channel_id: ChannelId,
    pub author_name: *const c_char,
    pub text: *const c_char,
    pub attachment_count: u8,
  }

  #[repr(u8)]
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum ChatDecisionCode {
    Continue = 0,
    Reject = 1,
    ReplaceText = 2,
  }

  impl ChatDecisionCode {
    pub const fn from_u8(value: u8) -> Option<Self> {
      match value {
        0 => Some(Self::Continue),
        1 => Some(Self::Reject),
        2 => Some(Self::ReplaceText),
        _ => None,
      }
    }
  }

  #[repr(C)]
  #[derive(Debug, Clone, Copy)]
  pub struct ChatDecision {
    pub abi: AbiHeader,
    pub code: u8,
    pub replacement_text: *const c_char,
    pub rejection_reason: *const c_char,
  }

  impl ChatDecision {
    pub const fn continue_() -> Self {
      Self {
        abi: AbiHeader::new::<Self>(),
        code: ChatDecisionCode::Continue as u8,
        replacement_text: core::ptr::null(),
        rejection_reason: core::ptr::null(),
      }
    }

    pub const fn reject(rejection_reason: *const c_char) -> Self {
      Self {
        abi: AbiHeader::new::<Self>(),
        code: ChatDecisionCode::Reject as u8,
        replacement_text: core::ptr::null(),
        rejection_reason,
      }
    }

    pub const fn replace_text(replacement_text: *const c_char) -> Self {
      Self {
        abi: AbiHeader::new::<Self>(),
        code: ChatDecisionCode::ReplaceText as u8,
        replacement_text,
        rejection_reason: core::ptr::null(),
      }
    }
  }

  pub type LogFn = unsafe extern "C" fn(context: *mut c_void, level: u8, message: *const c_char);
  pub type NowMsFn = unsafe extern "C" fn(context: *mut c_void) -> u64;
  pub type CreateChatCommandsFn =
    unsafe extern "C" fn(context: *mut c_void, commands: *const CommandDefinition, command_count: usize) -> bool;
  pub type CreateBotUserFn = unsafe extern "C" fn(
    context: *mut c_void,
    key: *const c_char,
    display_name: *const c_char,
    out_bot: *mut BotHandle,
    out_user_id: *mut UserId,
  ) -> bool;
  pub type DestroyBotUserFn = unsafe extern "C" fn(context: *mut c_void, bot: BotHandle) -> bool;
  pub type SetBotDisplayNameFn =
    unsafe extern "C" fn(context: *mut c_void, bot: BotHandle, display_name: *const c_char) -> bool;
  pub type SendBotChatFn = unsafe extern "C" fn(
    context: *mut c_void,
    bot: BotHandle,
    text_channel_id: ChannelId,
    text: *const c_char,
    out_message_id: *mut MessageId,
  ) -> bool;
  pub type JoinBotVoiceFn =
    unsafe extern "C" fn(context: *mut c_void, bot: BotHandle, voice_channel_id: ChannelId) -> bool;
  pub type LeaveBotVoiceFn = unsafe extern "C" fn(context: *mut c_void, bot: BotHandle) -> bool;
  pub type SendBotVoicePacketFn = unsafe extern "C" fn(
    context: *mut c_void,
    bot: BotHandle,
    sequence: u16,
    opus_payload: *const u8,
    opus_payload_len: usize,
  ) -> bool;
  pub type UserVoiceChannelFn =
    unsafe extern "C" fn(context: *mut c_void, user_id: UserId, out_voice_channel_id: *mut ChannelId) -> bool;
  pub type GetSessionInfoFn =
    unsafe extern "C" fn(context: *mut c_void, session: SessionId, out_info: *mut SessionInfo) -> bool;
  pub type GetUserInfoFn = unsafe extern "C" fn(context: *mut c_void, user_id: UserId, out_info: *mut UserInfo) -> bool;
  pub type FindUserByNameFn =
    unsafe extern "C" fn(context: *mut c_void, display_name: *const c_char, out_user_id: *mut UserId) -> bool;
  pub type GetChannelInfoFn =
    unsafe extern "C" fn(context: *mut c_void, channel_id: ChannelId, out_info: *mut ChannelInfo) -> bool;
  pub type ListChannelsFn =
    unsafe extern "C" fn(context: *mut c_void, out_channels: *mut ChannelInfo, inout_count: *mut usize) -> bool;
  pub type BotVoiceChannelFn =
    unsafe extern "C" fn(context: *mut c_void, bot: BotHandle, out_voice_channel_id: *mut ChannelId) -> bool;
  pub type MoveBotToUserVoiceFn = unsafe extern "C" fn(context: *mut c_void, bot: BotHandle, user_id: UserId) -> bool;
  pub type RespondToCommandQueryFn = unsafe extern "C" fn(
    context: *mut c_void,
    session_id: SessionId,
    request_id: u64,
    command_name: *const c_char,
    argument_name: *const c_char,
    response: *const CommandQueryResponse,
  ) -> bool;

  #[repr(C)]
  #[derive(Clone, Copy)]
  pub struct Host {
    pub abi: AbiHeader,
    pub context: *mut c_void,
    pub log: Option<LogFn>,
    pub now_ms: Option<NowMsFn>,
    pub create_chat_commands: Option<CreateChatCommandsFn>,
    pub create_bot_user: Option<CreateBotUserFn>,
    pub destroy_bot_user: Option<DestroyBotUserFn>,
    pub set_bot_display_name: Option<SetBotDisplayNameFn>,
    pub send_bot_chat: Option<SendBotChatFn>,
    pub join_bot_voice: Option<JoinBotVoiceFn>,
    pub leave_bot_voice: Option<LeaveBotVoiceFn>,
    pub send_bot_voice_packet: Option<SendBotVoicePacketFn>,
    pub user_voice_channel: Option<UserVoiceChannelFn>,
    pub get_session_info: Option<GetSessionInfoFn>,
    pub get_user_info: Option<GetUserInfoFn>,
    pub find_user_by_name: Option<FindUserByNameFn>,
    pub get_voice_channel_info: Option<GetChannelInfoFn>,
    pub get_text_channel_info: Option<GetChannelInfoFn>,
    pub list_voice_channels: Option<ListChannelsFn>,
    pub list_text_channels: Option<ListChannelsFn>,
    pub bot_voice_channel: Option<BotVoiceChannelFn>,
    pub move_bot_to_user_voice: Option<MoveBotToUserVoiceFn>,
    pub respond_to_command_query: Option<RespondToCommandQueryFn>,
    pub variables: *const PluginVariable,
    pub variable_count: usize,
  }

  impl Host {
    pub const fn empty() -> Self {
      Self {
        abi: AbiHeader::new::<Self>(),
        context: core::ptr::null_mut(),
        log: None,
        now_ms: None,
        create_chat_commands: None,
        create_bot_user: None,
        destroy_bot_user: None,
        set_bot_display_name: None,
        send_bot_chat: None,
        join_bot_voice: None,
        leave_bot_voice: None,
        send_bot_voice_packet: None,
        user_voice_channel: None,
        get_session_info: None,
        get_user_info: None,
        find_user_by_name: None,
        get_voice_channel_info: None,
        get_text_channel_info: None,
        list_voice_channels: None,
        list_text_channels: None,
        bot_voice_channel: None,
        move_bot_to_user_voice: None,
        respond_to_command_query: None,
        variables: core::ptr::null(),
        variable_count: 0,
      }
    }
  }

  pub type ServerStartedCallback = unsafe extern "C" fn();
  pub type ServerStoppingCallback = unsafe extern "C" fn();
  pub type SessionAuthenticatedCallback = unsafe extern "C" fn(session: SessionId);
  pub type SessionDisconnectedCallback =
    unsafe extern "C" fn(session: SessionId, user_id: UserId, voice_channel_id: ChannelId);
  pub type ChatMessageCallback = unsafe extern "C" fn(message: *const ChatMessage, decision: *mut ChatDecision);
  pub type ChatCommandCallback = unsafe extern "C" fn(invocation: *const ChatCommandInvocation);
  pub type ChatCommandQueryCallback =
    unsafe extern "C" fn(request: *const CommandQueryRequest, response: *mut CommandQueryResponse);

  #[repr(C)]
  #[derive(Clone, Copy)]
  pub struct Registration {
    pub abi: AbiHeader,
    pub on_server_started: Option<ServerStartedCallback>,
    pub on_server_stopping: Option<ServerStoppingCallback>,
    pub on_session_authenticated: Option<SessionAuthenticatedCallback>,
    pub on_session_disconnected: Option<SessionDisconnectedCallback>,
    pub on_chat_message: Option<ChatMessageCallback>,
    pub on_chat_command: Option<ChatCommandCallback>,
    pub on_chat_command_query: Option<ChatCommandQueryCallback>,
  }

  impl Registration {
    pub const fn empty() -> Self {
      Self {
        abi: AbiHeader::new::<Self>(),
        on_server_started: None,
        on_server_stopping: None,
        on_session_authenticated: None,
        on_session_disconnected: None,
        on_chat_message: None,
        on_chat_command: None,
        on_chat_command_query: None,
      }
    }
  }

  pub type InitFn = unsafe extern "C" fn(host: *const Host, registration: *mut Registration) -> bool;
  pub type ShutdownFn = unsafe extern "C" fn();
}

use std::{
  cell::RefCell,
  ffi::{CStr, CString, NulError},
  str::Utf8Error,
};

pub use abi::{BotHandle, ChannelId, MessageId, SessionId, UserId};

pub const CHAT_COMMAND_LIST_MESSAGE_TYPE: u16 = 0x0509;
pub const BOT_VOICE_SAMPLE_RATE_HZ: u32 = 48_000;
pub const BOT_VOICE_FRAME_DURATION_MS: u16 = 20;
pub const BOT_VOICE_CHANNELS: usize = 1;
pub const BOT_VOICE_FRAME_SAMPLES: usize =
  BOT_VOICE_SAMPLE_RATE_HZ as usize * BOT_VOICE_FRAME_DURATION_MS as usize / 1_000;
pub const BOT_VOICE_MAX_OPUS_PACKET_BYTES: usize = 512;
pub const BOT_VOICE_BITRATE_BPS: i32 = 64_000;

#[derive(Debug)]
pub enum PluginError {
  NullPointer(&'static str),
  IncompatibleAbi {
    ty: &'static str,
    expected_size: usize,
    actual_size: u32,
    expected_major: u16,
    actual_major: u16,
  },
  MissingHostFunction(&'static str),
  MissingPluginVariable(&'static str),
  HostCallFailed(&'static str),
  InvalidCommandName(String),
  InvalidCommandArgumentName(String),
  UnknownCommandArgType(u8),
  UnknownCommandInputMode(u8),
  UnknownCommandQueryStatus(u8),
  NulByte(NulError),
  Utf8(Utf8Error),
}

impl std::fmt::Display for PluginError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::NullPointer(name) => write!(f, "{name} pointer was null"),
      Self::IncompatibleAbi {
        ty,
        expected_size,
        actual_size,
        expected_major,
        actual_major,
      } => write!(
        f,
        "{ty} ABI mismatch: expected major {expected_major} and size >= {expected_size}, got major {actual_major} and size {actual_size}"
      ),
      Self::MissingHostFunction(name) => write!(f, "host function {name} is not available"),
      Self::MissingPluginVariable(name) => write!(f, "plugin variable {name} is missing"),
      Self::HostCallFailed(name) => write!(f, "host function {name} returned false"),
      Self::InvalidCommandName(name) => write!(f, "invalid command name: {name}"),
      Self::InvalidCommandArgumentName(name) => write!(f, "invalid command argument name: {name}"),
      Self::UnknownCommandArgType(value) => write!(f, "unknown command argument type: {value}"),
      Self::UnknownCommandInputMode(value) => write!(f, "unknown command input mode: {value}"),
      Self::UnknownCommandQueryStatus(value) => write!(f, "unknown command query status: {value}"),
      Self::NulByte(error) => write!(f, "string contains an interior nul byte: {error}"),
      Self::Utf8(error) => write!(f, "string is not valid UTF-8: {error}"),
    }
  }
}

impl std::error::Error for PluginError {}

impl From<NulError> for PluginError {
  fn from(value: NulError) -> Self {
    Self::NulByte(value)
  }
}

impl From<Utf8Error> for PluginError {
  fn from(value: Utf8Error) -> Self {
    Self::Utf8(value)
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginPermission {
  ReadSessions,
  ReadUsers,
  ReadChannels,
  ReadChat,
  ModerateChat,
  CreateChatCommands,
  CreateBotUsers,
  SendBotChat,
  JoinBotVoice,
  SendBotAudio,
}

impl PluginPermission {
  pub const fn as_str(self) -> &'static str {
    match self {
      Self::ReadSessions => "read_sessions",
      Self::ReadUsers => "read_users",
      Self::ReadChannels => "read_channels",
      Self::ReadChat => "read_chat",
      Self::ModerateChat => "moderate_chat",
      Self::CreateChatCommands => "create_chat_commands",
      Self::CreateBotUsers => "create_bot_users",
      Self::SendBotChat => "send_bot_chat",
      Self::JoinBotVoice => "join_bot_voice",
      Self::SendBotAudio => "send_bot_audio",
    }
  }

  pub const fn from_str(value: &str) -> Option<Self> {
    match value.as_bytes() {
      b"read_sessions" => Some(Self::ReadSessions),
      b"read_users" => Some(Self::ReadUsers),
      b"read_channels" => Some(Self::ReadChannels),
      b"read_chat" => Some(Self::ReadChat),
      b"moderate_chat" => Some(Self::ModerateChat),
      b"create_chat_commands" => Some(Self::CreateChatCommands),
      b"create_bot_users" => Some(Self::CreateBotUsers),
      b"send_bot_chat" => Some(Self::SendBotChat),
      b"join_bot_voice" => Some(Self::JoinBotVoice),
      b"send_bot_audio" => Some(Self::SendBotAudio),
      _ => None,
    }
  }

  pub const fn is_implemented(self) -> bool {
    true
  }
}

pub const IMPLEMENTED_PERMISSIONS: &[PluginPermission] = &[
  PluginPermission::ReadSessions,
  PluginPermission::ReadUsers,
  PluginPermission::ReadChannels,
  PluginPermission::ReadChat,
  PluginPermission::ModerateChat,
  PluginPermission::CreateChatCommands,
  PluginPermission::CreateBotUsers,
  PluginPermission::SendBotChat,
  PluginPermission::JoinBotVoice,
  PluginPermission::SendBotAudio,
];

pub const PLANNED_PERMISSIONS: &[PluginPermission] = &[];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginManifest {
  pub id: String,
  pub name: String,
  pub version: String,
  pub api_version: String,
  pub library: String,
  pub variables: Vec<(String, String)>,
  pub permissions: Vec<PluginPermission>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginConfig {
  pub enabled: bool,
  pub directory: String,
  pub allow: Vec<PluginAllowConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginAllowConfig {
  pub id: String,
  pub enabled: bool,
  pub permissions: Vec<PluginPermission>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandDefinition {
  pub name: String,
  pub description: String,
  pub usage: String,
  pub min_role: u8,
  pub inputs: Vec<CommandInputDefinition>,
}

impl CommandDefinition {
  pub fn new(name: impl Into<String>, description: impl Into<String>, usage: impl Into<String>) -> Self {
    Self {
      name: name.into(),
      description: description.into(),
      usage: usage.into(),
      min_role: 3,
      inputs: Vec::new(),
    }
  }

  pub fn with_min_role(mut self, min_role: u8) -> Self {
    self.min_role = min_role;
    self
  }

  pub fn with_input(mut self, input: CommandInputDefinition) -> Self {
    self.inputs.push(input);
    self
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandInputDefinition {
  pub argument_name: String,
  pub mode: abi::CommandInputMode,
  pub min_chars: u16,
  pub debounce_ms: u16,
  pub max_results: u16,
  pub placeholder: String,
}

impl CommandInputDefinition {
  pub fn live_query(argument_name: impl Into<String>) -> Self {
    Self {
      argument_name: argument_name.into(),
      mode: abi::CommandInputMode::LiveQuery,
      min_chars: 1,
      debounce_ms: 250,
      max_results: 10,
      placeholder: String::new(),
    }
  }

  pub fn with_min_chars(mut self, min_chars: u16) -> Self {
    self.min_chars = min_chars;
    self
  }

  pub fn with_debounce_ms(mut self, debounce_ms: u16) -> Self {
    self.debounce_ms = debounce_ms;
    self
  }

  pub fn with_max_results(mut self, max_results: u16) -> Self {
    self.max_results = max_results;
    self
  }

  pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
    self.placeholder = placeholder.into();
    self
  }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatCommandInvocation {
  pub session_id: SessionId,
  pub user_id: UserId,
  pub text_channel_id: ChannelId,
  pub caller_role: u8,
  pub command_name: String,
  pub args: String,
  pub raw_text: String,
  pub parsed_args: Vec<CommandArgumentValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommandArgumentValue {
  pub name: String,
  pub type_: abi::CommandArgType,
  pub present: bool,
  pub i64_value: i64,
  pub u64_value: u64,
  pub f64_value: f64,
  pub bool_value: bool,
  pub string_value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandQueryRequest {
  pub session_id: SessionId,
  pub user_id: UserId,
  pub text_channel_id: ChannelId,
  pub caller_role: u8,
  pub request_id: u64,
  pub command_name: String,
  pub argument_name: String,
  pub query: String,
  pub cursor_pos: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandQueryResult {
  pub id: String,
  pub title: String,
  pub subtitle: String,
  pub value: String,
  pub kind: String,
  pub duration_ms: u32,
  pub thumbnail_url: String,
}

impl CommandQueryResult {
  pub fn new(title: impl Into<String>, value: impl Into<String>) -> Self {
    Self {
      id: String::new(),
      title: title.into(),
      subtitle: String::new(),
      value: value.into(),
      kind: String::new(),
      duration_ms: 0,
      thumbnail_url: String::new(),
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandQueryResponse {
  pub status: abi::CommandQueryStatus,
  pub message: String,
  pub results: Vec<CommandQueryResult>,
}

impl CommandQueryResponse {
  pub fn ok(results: Vec<CommandQueryResult>) -> Self {
    let status = if results.is_empty() {
      abi::CommandQueryStatus::NoResults
    } else {
      abi::CommandQueryStatus::Ok
    };
    Self {
      status,
      message: String::new(),
      results,
    }
  }

  pub fn no_results(message: impl Into<String>) -> Self {
    Self {
      status: abi::CommandQueryStatus::NoResults,
      message: message.into(),
      results: Vec::new(),
    }
  }

  pub fn plugin_error(message: impl Into<String>) -> Self {
    Self {
      status: abi::CommandQueryStatus::PluginError,
      message: message.into(),
      results: Vec::new(),
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginVariableRef<'a> {
  pub key: &'a str,
  pub value: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
  pub session_id: SessionId,
  pub user_id: UserId,
  pub voice_channel_id: ChannelId,
  pub role: u8,
  pub authenticated: bool,
  pub muted: bool,
  pub deafened: bool,
  pub username: String,
}

impl TryFrom<abi::SessionInfo> for SessionInfo {
  type Error = PluginError;

  fn try_from(value: abi::SessionInfo) -> Result<Self, Self::Error> {
    Ok(Self {
      session_id: value.session_id,
      user_id: value.user_id,
      voice_channel_id: value.voice_channel_id,
      role: value.role,
      authenticated: value.authenticated != 0,
      muted: value.muted != 0,
      deafened: value.deafened != 0,
      username: fixed_cstr(&value.username)?.to_owned(),
    })
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserInfo {
  pub user_id: UserId,
  pub role: u8,
  pub is_bot: bool,
  pub display_name: String,
  pub fingerprint: String,
  pub bot_owner_plugin: String,
  pub bot_key: String,
}

impl TryFrom<abi::UserInfo> for UserInfo {
  type Error = PluginError;

  fn try_from(value: abi::UserInfo) -> Result<Self, Self::Error> {
    Ok(Self {
      user_id: value.user_id,
      role: value.role,
      is_bot: value.is_bot != 0,
      display_name: fixed_cstr(&value.display_name)?.to_owned(),
      fingerprint: fixed_cstr(&value.fingerprint)?.to_owned(),
      bot_owner_plugin: fixed_cstr(&value.bot_owner_plugin)?.to_owned(),
      bot_key: fixed_cstr(&value.bot_key)?.to_owned(),
    })
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelInfo {
  pub channel_id: ChannelId,
  pub user_count: u32,
  pub max_users: i32,
  pub sort_order: i32,
  pub name: String,
}

impl TryFrom<abi::ChannelInfo> for ChannelInfo {
  type Error = PluginError;

  fn try_from(value: abi::ChannelInfo) -> Result<Self, Self::Error> {
    Ok(Self {
      channel_id: value.channel_id,
      user_count: value.user_count,
      max_users: value.max_users,
      sort_order: value.sort_order,
      name: fixed_cstr(&value.name)?.to_owned(),
    })
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
  pub session_id: SessionId,
  pub author_user_id: UserId,
  pub text_channel_id: ChannelId,
  pub author_name: String,
  pub text: String,
  pub attachment_count: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatDecision {
  Continue,
  Reject { reason: String },
  ReplaceText { text: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotUser {
  handle: BotHandle,
  user_id: UserId,
  key: String,
  display_name: String,
}

impl BotUser {
  pub fn user_id(&self) -> UserId {
    self.user_id
  }

  pub fn key(&self) -> &str {
    &self.key
  }

  pub fn display_name(&self) -> &str {
    &self.display_name
  }

  pub fn raw_handle(&self) -> BotHandle {
    self.handle
  }
}

// Bot handles are opaque server-owned handles. The server API allows plugins to
// copy them and pass them back to host calls during the current server run.
unsafe impl Send for BotUser {}
unsafe impl Sync for BotUser {}

#[derive(Clone, Copy)]
pub struct HostRef<'a> {
  inner: &'a abi::Host,
}

impl<'a> HostRef<'a> {
  /// Creates a host wrapper from the raw pointer passed to `parties_plugin_init`.
  ///
  /// # Safety
  ///
  /// `host` must point to a valid `abi::Host` for the lifetime of the returned
  /// wrapper. This is guaranteed by the Parties server during plugin init.
  pub unsafe fn from_raw(host: *const abi::Host) -> Result<Self, PluginError> {
    if host.is_null() {
      return Err(PluginError::NullPointer("host"));
    }

    let inner = unsafe { &*host };
    require_compatible_abi::<abi::Host>("Host", inner.abi)?;
    Ok(Self { inner })
  }

  pub fn raw(&self) -> &'a abi::Host {
    self.inner
  }

  pub fn variables(&self) -> Result<Vec<PluginVariableRef<'a>>, PluginError> {
    if self.inner.variable_count == 0 {
      return Ok(Vec::new());
    }
    if self.inner.variables.is_null() {
      return Err(PluginError::NullPointer("variables"));
    }

    let variables = unsafe { std::slice::from_raw_parts(self.inner.variables, self.inner.variable_count) };
    variables
      .iter()
      .map(|variable| {
        require_compatible_abi::<abi::PluginVariable>("PluginVariable", variable.abi)?;
        Ok(PluginVariableRef {
          key: required_cstr(variable.key, "variable.key")?,
          value: required_cstr(variable.value, "variable.value")?,
        })
      })
      .collect()
  }

  pub fn to_handle(&self) -> HostHandle {
    HostHandle { inner: *self.inner }
  }

  pub fn log(&self, level: abi::LogLevel, message: &str) -> Result<(), PluginError> {
    let log = self.inner.log.ok_or(PluginError::MissingHostFunction("log"))?;
    let message = CString::new(message)?;
    unsafe {
      log(self.inner.context, level as u8, message.as_ptr());
    }
    Ok(())
  }

  pub fn now_ms(&self) -> Result<u64, PluginError> {
    let now_ms = self.inner.now_ms.ok_or(PluginError::MissingHostFunction("now_ms"))?;
    Ok(unsafe { now_ms(self.inner.context) })
  }

  pub fn create_chat_commands(&self, commands: &[CommandDefinition]) -> Result<(), PluginError> {
    let create_chat_commands = self
      .inner
      .create_chat_commands
      .ok_or(PluginError::MissingHostFunction("create_chat_commands"))?;

    for command in commands {
      if !is_valid_command_name(&command.name) {
        return Err(PluginError::InvalidCommandName(command.name.clone()));
      }
      for input in &command.inputs {
        if !is_valid_command_argument_name(&input.argument_name) {
          return Err(PluginError::InvalidCommandArgumentName(input.argument_name.clone()));
        }
      }
    }

    let names = cstrings(commands.iter().map(|command| command.name.as_str()))?;
    let descriptions = cstrings(commands.iter().map(|command| command.description.as_str()))?;
    let usages = cstrings(commands.iter().map(|command| command.usage.as_str()))?;
    let input_name_storage = commands
      .iter()
      .map(|command| cstrings(command.inputs.iter().map(|input| input.argument_name.as_str())))
      .collect::<Result<Vec<_>, _>>()?;
    let input_placeholder_storage = commands
      .iter()
      .map(|command| cstrings(command.inputs.iter().map(|input| input.placeholder.as_str())))
      .collect::<Result<Vec<_>, _>>()?;
    let abi_inputs = commands
      .iter()
      .enumerate()
      .map(|(command_index, command)| {
        command
          .inputs
          .iter()
          .enumerate()
          .map(|(input_index, input)| abi::CommandInputDefinition {
            abi: abi::AbiHeader::new::<abi::CommandInputDefinition>(),
            argument_name: input_name_storage[command_index][input_index].as_ptr(),
            mode: input.mode as u8,
            min_chars: input.min_chars,
            debounce_ms: input.debounce_ms,
            max_results: input.max_results,
            placeholder: input_placeholder_storage[command_index][input_index].as_ptr(),
          })
          .collect::<Vec<_>>()
      })
      .collect::<Vec<_>>();
    let abi_commands = commands
      .iter()
      .enumerate()
      .map(|(index, _)| abi::CommandDefinition {
        abi: abi::AbiHeader::new::<abi::CommandDefinition>(),
        name: names[index].as_ptr(),
        description: descriptions[index].as_ptr(),
        usage: usages[index].as_ptr(),
        min_role: commands[index].min_role,
        inputs: if abi_inputs[index].is_empty() {
          std::ptr::null()
        } else {
          abi_inputs[index].as_ptr()
        },
        input_count: abi_inputs[index].len(),
      })
      .collect::<Vec<_>>();

    if unsafe { create_chat_commands(self.inner.context, abi_commands.as_ptr(), abi_commands.len()) } {
      Ok(())
    } else {
      Err(PluginError::HostCallFailed("create_chat_commands"))
    }
  }

  pub fn create_bot_user(&self, key: &str, display_name: &str) -> Result<BotUser, PluginError> {
    let create_bot_user = self
      .inner
      .create_bot_user
      .ok_or(PluginError::MissingHostFunction("create_bot_user"))?;
    let key_c = CString::new(key)?;
    let display_name_c = CString::new(display_name)?;
    let mut handle = std::ptr::null_mut();
    let mut user_id = 0;

    if !unsafe {
      create_bot_user(
        self.inner.context,
        key_c.as_ptr(),
        display_name_c.as_ptr(),
        &mut handle,
        &mut user_id,
      )
    } {
      return Err(PluginError::HostCallFailed("create_bot_user"));
    }
    if handle.is_null() {
      return Err(PluginError::NullPointer("bot"));
    }

    Ok(BotUser {
      handle,
      user_id,
      key: key.to_owned(),
      display_name: display_name.to_owned(),
    })
  }

  pub fn destroy_bot_user(&self, bot: &BotUser) -> Result<(), PluginError> {
    let destroy_bot_user = self
      .inner
      .destroy_bot_user
      .ok_or(PluginError::MissingHostFunction("destroy_bot_user"))?;
    if unsafe { destroy_bot_user(self.inner.context, bot.handle) } {
      Ok(())
    } else {
      Err(PluginError::HostCallFailed("destroy_bot_user"))
    }
  }

  pub fn set_bot_display_name(&self, bot: &mut BotUser, display_name: &str) -> Result<(), PluginError> {
    let set_bot_display_name = self
      .inner
      .set_bot_display_name
      .ok_or(PluginError::MissingHostFunction("set_bot_display_name"))?;
    let display_name = CString::new(display_name)?;
    if unsafe { set_bot_display_name(self.inner.context, bot.handle, display_name.as_ptr()) } {
      bot.display_name = display_name.to_string_lossy().into_owned();
      Ok(())
    } else {
      Err(PluginError::HostCallFailed("set_bot_display_name"))
    }
  }

  pub fn send_bot_chat(&self, bot: &BotUser, text_channel_id: ChannelId, text: &str) -> Result<MessageId, PluginError> {
    let send_bot_chat = self
      .inner
      .send_bot_chat
      .ok_or(PluginError::MissingHostFunction("send_bot_chat"))?;
    let text = CString::new(text)?;
    let mut message_id = 0;
    if unsafe {
      send_bot_chat(
        self.inner.context,
        bot.handle,
        text_channel_id,
        text.as_ptr(),
        &mut message_id,
      )
    } {
      Ok(message_id)
    } else {
      Err(PluginError::HostCallFailed("send_bot_chat"))
    }
  }

  pub fn join_bot_voice(&self, bot: &BotUser, voice_channel_id: ChannelId) -> Result<(), PluginError> {
    let join_bot_voice = self
      .inner
      .join_bot_voice
      .ok_or(PluginError::MissingHostFunction("join_bot_voice"))?;
    if unsafe { join_bot_voice(self.inner.context, bot.handle, voice_channel_id) } {
      Ok(())
    } else {
      Err(PluginError::HostCallFailed("join_bot_voice"))
    }
  }

  pub fn leave_bot_voice(&self, bot: &BotUser) -> Result<(), PluginError> {
    let leave_bot_voice = self
      .inner
      .leave_bot_voice
      .ok_or(PluginError::MissingHostFunction("leave_bot_voice"))?;
    if unsafe { leave_bot_voice(self.inner.context, bot.handle) } {
      Ok(())
    } else {
      Err(PluginError::HostCallFailed("leave_bot_voice"))
    }
  }

  pub fn send_bot_voice_packet(&self, bot: &BotUser, sequence: u16, opus_payload: &[u8]) -> Result<(), PluginError> {
    let send_bot_voice_packet = self
      .inner
      .send_bot_voice_packet
      .ok_or(PluginError::MissingHostFunction("send_bot_voice_packet"))?;
    if unsafe {
      send_bot_voice_packet(
        self.inner.context,
        bot.handle,
        sequence,
        opus_payload.as_ptr(),
        opus_payload.len(),
      )
    } {
      Ok(())
    } else {
      Err(PluginError::HostCallFailed("send_bot_voice_packet"))
    }
  }

  pub fn user_voice_channel(&self, user_id: UserId) -> Result<Option<ChannelId>, PluginError> {
    let user_voice_channel = self
      .inner
      .user_voice_channel
      .ok_or(PluginError::MissingHostFunction("user_voice_channel"))?;
    let mut voice_channel_id = 0;
    if !unsafe { user_voice_channel(self.inner.context, user_id, &mut voice_channel_id) } {
      return Err(PluginError::HostCallFailed("user_voice_channel"));
    }

    Ok((voice_channel_id != 0).then_some(voice_channel_id))
  }

  pub fn get_session_info(&self, session: SessionId) -> Result<SessionInfo, PluginError> {
    let get_session_info = self
      .inner
      .get_session_info
      .ok_or(PluginError::MissingHostFunction("get_session_info"))?;
    let mut info = abi::SessionInfo::default();
    if !unsafe { get_session_info(self.inner.context, session, &mut info) } {
      return Err(PluginError::HostCallFailed("get_session_info"));
    }
    SessionInfo::try_from(info)
  }

  pub fn get_user_info(&self, user_id: UserId) -> Result<UserInfo, PluginError> {
    let get_user_info = self
      .inner
      .get_user_info
      .ok_or(PluginError::MissingHostFunction("get_user_info"))?;
    let mut info = abi::UserInfo::default();
    if !unsafe { get_user_info(self.inner.context, user_id, &mut info) } {
      return Err(PluginError::HostCallFailed("get_user_info"));
    }
    UserInfo::try_from(info)
  }

  pub fn find_user_by_name(&self, display_name: &str) -> Result<UserId, PluginError> {
    let find_user_by_name = self
      .inner
      .find_user_by_name
      .ok_or(PluginError::MissingHostFunction("find_user_by_name"))?;
    let display_name = CString::new(display_name)?;
    let mut user_id = 0;
    if unsafe { find_user_by_name(self.inner.context, display_name.as_ptr(), &mut user_id) } {
      Ok(user_id)
    } else {
      Err(PluginError::HostCallFailed("find_user_by_name"))
    }
  }

  pub fn get_voice_channel_info(&self, channel_id: ChannelId) -> Result<ChannelInfo, PluginError> {
    self.get_channel_info(channel_id, self.inner.get_voice_channel_info, "get_voice_channel_info")
  }

  pub fn get_text_channel_info(&self, channel_id: ChannelId) -> Result<ChannelInfo, PluginError> {
    self.get_channel_info(channel_id, self.inner.get_text_channel_info, "get_text_channel_info")
  }

  pub fn list_voice_channels(&self) -> Result<Vec<ChannelInfo>, PluginError> {
    self.list_channels(self.inner.list_voice_channels, "list_voice_channels")
  }

  pub fn list_text_channels(&self) -> Result<Vec<ChannelInfo>, PluginError> {
    self.list_channels(self.inner.list_text_channels, "list_text_channels")
  }

  pub fn bot_voice_channel(&self, bot: &BotUser) -> Result<Option<ChannelId>, PluginError> {
    let bot_voice_channel = self
      .inner
      .bot_voice_channel
      .ok_or(PluginError::MissingHostFunction("bot_voice_channel"))?;
    let mut voice_channel_id = 0;
    if !unsafe { bot_voice_channel(self.inner.context, bot.handle, &mut voice_channel_id) } {
      return Err(PluginError::HostCallFailed("bot_voice_channel"));
    }

    Ok((voice_channel_id != 0).then_some(voice_channel_id))
  }

  pub fn move_bot_to_user_voice(&self, bot: &BotUser, user_id: UserId) -> Result<(), PluginError> {
    let move_bot_to_user_voice = self
      .inner
      .move_bot_to_user_voice
      .ok_or(PluginError::MissingHostFunction("move_bot_to_user_voice"))?;
    if unsafe { move_bot_to_user_voice(self.inner.context, bot.handle, user_id) } {
      Ok(())
    } else {
      Err(PluginError::HostCallFailed("move_bot_to_user_voice"))
    }
  }

  pub fn respond_to_command_query(
    &self,
    request: &CommandQueryRequestRef<'_>,
    response: CommandQueryResponse,
  ) -> Result<(), PluginError> {
    let respond_to_command_query = self
      .inner
      .respond_to_command_query
      .ok_or(PluginError::MissingHostFunction("respond_to_command_query"))?;
    let command_name = CString::new(request.command_name)?;
    let argument_name = CString::new(request.argument_name)?;
    with_abi_command_query_response(response, |abi_response| {
      if unsafe {
        respond_to_command_query(
          self.inner.context,
          request.session_id,
          request.request_id,
          command_name.as_ptr(),
          argument_name.as_ptr(),
          abi_response,
        )
      } {
        Ok(())
      } else {
        Err(PluginError::HostCallFailed("respond_to_command_query"))
      }
    })
  }

  fn get_channel_info(
    &self,
    channel_id: ChannelId,
    call: Option<abi::GetChannelInfoFn>,
    name: &'static str,
  ) -> Result<ChannelInfo, PluginError> {
    let call = call.ok_or(PluginError::MissingHostFunction(name))?;
    let mut info = abi::ChannelInfo::default();
    if !unsafe { call(self.inner.context, channel_id, &mut info) } {
      return Err(PluginError::HostCallFailed(name));
    }
    ChannelInfo::try_from(info)
  }

  fn list_channels(
    &self,
    call: Option<abi::ListChannelsFn>,
    name: &'static str,
  ) -> Result<Vec<ChannelInfo>, PluginError> {
    let call = call.ok_or(PluginError::MissingHostFunction(name))?;
    let mut count = 0;
    unsafe {
      call(self.inner.context, std::ptr::null_mut(), &mut count);
    }
    if count == 0 {
      return Ok(Vec::new());
    }

    let mut channels = vec![abi::ChannelInfo::default(); count];
    let mut capacity = channels.len();
    if !unsafe { call(self.inner.context, channels.as_mut_ptr(), &mut capacity) } {
      return Err(PluginError::HostCallFailed(name));
    }

    channels
      .into_iter()
      .map(ChannelInfo::try_from)
      .collect::<Result<Vec<_>, _>>()
  }
}

#[derive(Clone, Copy)]
pub struct HostHandle {
  inner: abi::Host,
}

// The native plugin ABI is designed for plugins to copy the host table and use
// it after init. The context pointer remains server-owned and opaque.
unsafe impl Send for HostHandle {}
unsafe impl Sync for HostHandle {}

impl HostHandle {
  pub fn borrowed(&self) -> HostRef<'_> {
    HostRef { inner: &self.inner }
  }

  pub fn log(&self, level: abi::LogLevel, message: &str) -> Result<(), PluginError> {
    self.borrowed().log(level, message)
  }

  pub fn now_ms(&self) -> Result<u64, PluginError> {
    self.borrowed().now_ms()
  }

  pub fn create_chat_commands(&self, commands: &[CommandDefinition]) -> Result<(), PluginError> {
    self.borrowed().create_chat_commands(commands)
  }

  pub fn create_bot_user(&self, key: &str, display_name: &str) -> Result<BotUser, PluginError> {
    self.borrowed().create_bot_user(key, display_name)
  }

  pub fn destroy_bot_user(&self, bot: &BotUser) -> Result<(), PluginError> {
    self.borrowed().destroy_bot_user(bot)
  }

  pub fn set_bot_display_name(&self, bot: &mut BotUser, display_name: &str) -> Result<(), PluginError> {
    self.borrowed().set_bot_display_name(bot, display_name)
  }

  pub fn send_bot_chat(&self, bot: &BotUser, text_channel_id: ChannelId, text: &str) -> Result<MessageId, PluginError> {
    self.borrowed().send_bot_chat(bot, text_channel_id, text)
  }

  pub fn join_bot_voice(&self, bot: &BotUser, voice_channel_id: ChannelId) -> Result<(), PluginError> {
    self.borrowed().join_bot_voice(bot, voice_channel_id)
  }

  pub fn leave_bot_voice(&self, bot: &BotUser) -> Result<(), PluginError> {
    self.borrowed().leave_bot_voice(bot)
  }

  pub fn send_bot_voice_packet(&self, bot: &BotUser, sequence: u16, opus_payload: &[u8]) -> Result<(), PluginError> {
    self.borrowed().send_bot_voice_packet(bot, sequence, opus_payload)
  }

  pub fn user_voice_channel(&self, user_id: UserId) -> Result<Option<ChannelId>, PluginError> {
    self.borrowed().user_voice_channel(user_id)
  }

  pub fn variables(&self) -> Result<Vec<PluginVariableRef<'_>>, PluginError> {
    self.borrowed().variables()
  }

  pub fn get_session_info(&self, session: SessionId) -> Result<SessionInfo, PluginError> {
    self.borrowed().get_session_info(session)
  }

  pub fn get_user_info(&self, user_id: UserId) -> Result<UserInfo, PluginError> {
    self.borrowed().get_user_info(user_id)
  }

  pub fn find_user_by_name(&self, display_name: &str) -> Result<UserId, PluginError> {
    self.borrowed().find_user_by_name(display_name)
  }

  pub fn get_voice_channel_info(&self, channel_id: ChannelId) -> Result<ChannelInfo, PluginError> {
    self.borrowed().get_voice_channel_info(channel_id)
  }

  pub fn get_text_channel_info(&self, channel_id: ChannelId) -> Result<ChannelInfo, PluginError> {
    self.borrowed().get_text_channel_info(channel_id)
  }

  pub fn list_voice_channels(&self) -> Result<Vec<ChannelInfo>, PluginError> {
    self.borrowed().list_voice_channels()
  }

  pub fn list_text_channels(&self) -> Result<Vec<ChannelInfo>, PluginError> {
    self.borrowed().list_text_channels()
  }

  pub fn bot_voice_channel(&self, bot: &BotUser) -> Result<Option<ChannelId>, PluginError> {
    self.borrowed().bot_voice_channel(bot)
  }

  pub fn move_bot_to_user_voice(&self, bot: &BotUser, user_id: UserId) -> Result<(), PluginError> {
    self.borrowed().move_bot_to_user_voice(bot, user_id)
  }

  pub fn respond_to_command_query(
    &self,
    request: &CommandQueryRequestRef<'_>,
    response: CommandQueryResponse,
  ) -> Result<(), PluginError> {
    self.borrowed().respond_to_command_query(request, response)
  }
}

pub struct RegistrationWriter<'a> {
  inner: &'a mut abi::Registration,
}

impl<'a> RegistrationWriter<'a> {
  /// Creates a registration writer from the raw pointer passed to
  /// `parties_plugin_init`.
  ///
  /// # Safety
  ///
  /// `registration` must point to a valid mutable `abi::Registration` for the
  /// lifetime of the returned writer. This is guaranteed by the Parties server
  /// during plugin init.
  pub unsafe fn from_raw(registration: *mut abi::Registration) -> Result<Self, PluginError> {
    if registration.is_null() {
      return Err(PluginError::NullPointer("registration"));
    }

    unsafe {
      *registration = abi::Registration::empty();
    }
    Ok(Self {
      inner: unsafe { &mut *registration },
    })
  }

  pub fn set_on_server_started(&mut self, callback: abi::ServerStartedCallback) {
    self.inner.on_server_started = Some(callback);
  }

  pub fn set_on_server_stopping(&mut self, callback: abi::ServerStoppingCallback) {
    self.inner.on_server_stopping = Some(callback);
  }

  pub fn set_on_session_authenticated(&mut self, callback: abi::SessionAuthenticatedCallback) {
    self.inner.on_session_authenticated = Some(callback);
  }

  pub fn set_on_session_disconnected(&mut self, callback: abi::SessionDisconnectedCallback) {
    self.inner.on_session_disconnected = Some(callback);
  }

  pub fn set_on_chat_message(&mut self, callback: abi::ChatMessageCallback) {
    self.inner.on_chat_message = Some(callback);
  }

  pub fn set_on_chat_command(&mut self, callback: abi::ChatCommandCallback) {
    self.inner.on_chat_command = Some(callback);
  }

  pub fn set_on_chat_command_query(&mut self, callback: abi::ChatCommandQueryCallback) {
    self.inner.on_chat_command_query = Some(callback);
  }
}

pub mod plugin {
  use super::{
    ChannelInfo, ChatCommandInvocationRef, ChatDecision, ChatMessageRef, CommandDefinition, CommandQueryRequestRef,
    CommandQueryResponse, HostHandle, PluginError, PluginVariableRef, RegistrationWriter, SessionInfo, UserInfo, abi,
  };

  pub trait Plugin: Default + Send + 'static {
    fn init(&mut self, _context: &mut Context<'_>) -> Result<(), PluginError> {
      Ok(())
    }

    fn shutdown(&mut self) {}

    fn on_server_started(&mut self) {}

    fn on_server_stopping(&mut self) {}

    fn on_session_authenticated(&mut self, _session: abi::SessionId) {}

    fn on_session_disconnected(
      &mut self,
      _session: abi::SessionId,
      _user_id: abi::UserId,
      _voice_channel_id: abi::ChannelId,
    ) {
    }

    fn on_chat_command(&mut self, _invocation: ChatCommandInvocationRef<'_>) {}

    fn on_chat_command_query(&mut self, _request: CommandQueryRequestRef<'_>) -> CommandQueryResponse {
      CommandQueryResponse::no_results("")
    }

    fn on_chat_message(&mut self, _message: ChatMessageRef<'_>) -> ChatDecision {
      ChatDecision::Continue
    }
  }

  pub struct Context<'registration> {
    host: HostHandle,
    registration: RegistrationWriter<'registration>,
  }

  impl<'registration> Context<'registration> {
    pub fn new(host: HostHandle, registration: RegistrationWriter<'registration>) -> Self {
      Self { host, registration }
    }

    pub fn host(&self) -> HostHandle {
      self.host
    }

    pub fn registration(&mut self) -> &mut RegistrationWriter<'registration> {
      &mut self.registration
    }

    pub fn log(&self, level: abi::LogLevel, message: &str) -> Result<(), PluginError> {
      self.host.log(level, message)
    }

    pub fn now_ms(&self) -> Result<u64, PluginError> {
      self.host.now_ms()
    }

    pub fn register_commands(&self, commands: &[CommandDefinition]) -> Result<(), PluginError> {
      self.host.create_chat_commands(commands)
    }

    pub fn user_voice_channel(&self, user_id: abi::UserId) -> Result<Option<abi::ChannelId>, PluginError> {
      self.host.user_voice_channel(user_id)
    }

    pub fn variables(&self) -> Result<Vec<PluginVariableRef<'_>>, PluginError> {
      self.host.variables()
    }

    pub fn get_session_info(&self, session: abi::SessionId) -> Result<SessionInfo, PluginError> {
      self.host.get_session_info(session)
    }

    pub fn get_user_info(&self, user_id: abi::UserId) -> Result<UserInfo, PluginError> {
      self.host.get_user_info(user_id)
    }

    pub fn find_user_by_name(&self, display_name: &str) -> Result<abi::UserId, PluginError> {
      self.host.find_user_by_name(display_name)
    }

    pub fn get_voice_channel_info(&self, channel_id: abi::ChannelId) -> Result<ChannelInfo, PluginError> {
      self.host.get_voice_channel_info(channel_id)
    }

    pub fn get_text_channel_info(&self, channel_id: abi::ChannelId) -> Result<ChannelInfo, PluginError> {
      self.host.get_text_channel_info(channel_id)
    }

    pub fn list_voice_channels(&self) -> Result<Vec<ChannelInfo>, PluginError> {
      self.host.list_voice_channels()
    }

    pub fn list_text_channels(&self) -> Result<Vec<ChannelInfo>, PluginError> {
      self.host.list_text_channels()
    }

    pub fn bot_voice_channel(&self, bot: &super::BotUser) -> Result<Option<abi::ChannelId>, PluginError> {
      self.host.bot_voice_channel(bot)
    }

    pub fn move_bot_to_user_voice(&self, bot: &super::BotUser, user_id: abi::UserId) -> Result<(), PluginError> {
      self.host.move_bot_to_user_voice(bot, user_id)
    }

    pub fn respond_to_command_query(
      &self,
      request: &CommandQueryRequestRef<'_>,
      response: CommandQueryResponse,
    ) -> Result<(), PluginError> {
      self.host.respond_to_command_query(request, response)
    }
  }

  #[macro_export]
  macro_rules! register_plugin {
    ($plugin:ty) => {
      static PARTIES_PLUGIN_INSTANCE: std::sync::OnceLock<std::sync::Mutex<$plugin>> = std::sync::OnceLock::new();

      fn parties_plugin_instance() -> &'static std::sync::Mutex<$plugin> {
        PARTIES_PLUGIN_INSTANCE.get_or_init(|| std::sync::Mutex::new(<$plugin as Default>::default()))
      }

      /// Initializes the Parties plugin.
      ///
      /// # Safety
      ///
      /// The Parties server must pass valid `Host` and `Registration` pointers
      /// that remain alive for the duration of this call.
      #[unsafe(no_mangle)]
      pub unsafe extern "C" fn parties_plugin_init(
        host: *const $crate::abi::Host,
        registration: *mut $crate::abi::Registration,
      ) -> bool {
        let Ok(host) = (unsafe { $crate::HostRef::from_raw(host) }) else {
          return false;
        };
        let host = host.to_handle();
        let Ok(mut registration) = (unsafe { $crate::RegistrationWriter::from_raw(registration) }) else {
          return false;
        };

        registration.set_on_server_started(parties_plugin_on_server_started);
        registration.set_on_server_stopping(parties_plugin_on_server_stopping);
        registration.set_on_session_authenticated(parties_plugin_on_session_authenticated);
        registration.set_on_session_disconnected(parties_plugin_on_session_disconnected);
        registration.set_on_chat_message(parties_plugin_on_chat_message);
        registration.set_on_chat_command(parties_plugin_on_chat_command);
        registration.set_on_chat_command_query(parties_plugin_on_chat_command_query);

        let mut context = $crate::plugin::Context::new(host, registration);
        let Ok(mut plugin) = parties_plugin_instance().lock() else {
          return false;
        };

        <$plugin as $crate::plugin::Plugin>::init(&mut *plugin, &mut context).is_ok()
      }

      #[unsafe(no_mangle)]
      pub extern "C" fn parties_plugin_shutdown() {
        if let Some(plugin) = PARTIES_PLUGIN_INSTANCE.get() {
          if let Ok(mut plugin) = plugin.lock() {
            <$plugin as $crate::plugin::Plugin>::shutdown(&mut *plugin);
          }
        }
      }

      unsafe extern "C" fn parties_plugin_on_server_started() {
        if let Ok(mut plugin) = parties_plugin_instance().lock() {
          <$plugin as $crate::plugin::Plugin>::on_server_started(&mut *plugin);
        }
      }

      unsafe extern "C" fn parties_plugin_on_server_stopping() {
        if let Ok(mut plugin) = parties_plugin_instance().lock() {
          <$plugin as $crate::plugin::Plugin>::on_server_stopping(&mut *plugin);
        }
      }

      unsafe extern "C" fn parties_plugin_on_session_authenticated(session: $crate::abi::SessionId) {
        if let Ok(mut plugin) = parties_plugin_instance().lock() {
          <$plugin as $crate::plugin::Plugin>::on_session_authenticated(&mut *plugin, session);
        }
      }

      unsafe extern "C" fn parties_plugin_on_session_disconnected(
        session: $crate::abi::SessionId,
        user_id: $crate::abi::UserId,
        voice_channel_id: $crate::abi::ChannelId,
      ) {
        if let Ok(mut plugin) = parties_plugin_instance().lock() {
          <$plugin as $crate::plugin::Plugin>::on_session_disconnected(
            &mut *plugin,
            session,
            user_id,
            voice_channel_id,
          );
        }
      }

      unsafe extern "C" fn parties_plugin_on_chat_command(invocation: *const $crate::abi::ChatCommandInvocation) {
        let Ok(invocation) = (unsafe { $crate::ChatCommandInvocationRef::from_raw(invocation) }) else {
          return;
        };
        if let Ok(mut plugin) = parties_plugin_instance().lock() {
          <$plugin as $crate::plugin::Plugin>::on_chat_command(&mut *plugin, invocation);
        }
      }

      unsafe extern "C" fn parties_plugin_on_chat_message(
        message: *const $crate::abi::ChatMessage,
        decision: *mut $crate::abi::ChatDecision,
      ) {
        let Ok(message) = (unsafe { $crate::ChatMessageRef::from_raw(message) }) else {
          return;
        };
        if let Ok(mut plugin) = parties_plugin_instance().lock() {
          let result = <$plugin as $crate::plugin::Plugin>::on_chat_message(&mut *plugin, message);
          unsafe {
            $crate::write_chat_decision(decision, result);
          }
        }
      }

      unsafe extern "C" fn parties_plugin_on_chat_command_query(
        request: *const $crate::abi::CommandQueryRequest,
        response: *mut $crate::abi::CommandQueryResponse,
      ) {
        let Ok(request) = (unsafe { $crate::CommandQueryRequestRef::from_raw(request) }) else {
          return;
        };
        if let Ok(mut plugin) = parties_plugin_instance().lock() {
          let result = <$plugin as $crate::plugin::Plugin>::on_chat_command_query(&mut *plugin, request);
          unsafe {
            $crate::write_command_query_response(response, result);
          }
        }
      }
    };
  }

  pub use crate::register_plugin as register;
}

#[derive(Debug, Clone)]
pub struct ChatCommandInvocationRef<'a> {
  pub session_id: SessionId,
  pub user_id: UserId,
  pub text_channel_id: ChannelId,
  pub caller_role: u8,
  pub command_name: &'a str,
  pub args: &'a str,
  pub raw_text: &'a str,
  pub parsed_args: Vec<CommandArgumentValueRef<'a>>,
}

impl<'a> ChatCommandInvocationRef<'a> {
  /// Converts the ABI callback pointer into borrowed Rust strings.
  ///
  /// # Safety
  ///
  /// `invocation` must point to a valid ABI invocation whose string pointers are
  /// valid for the returned lifetime. The server guarantees this for the
  /// duration of `on_chat_command`.
  pub unsafe fn from_raw(invocation: *const abi::ChatCommandInvocation) -> Result<Self, PluginError> {
    if invocation.is_null() {
      return Err(PluginError::NullPointer("invocation"));
    }

    let invocation = unsafe { &*invocation };
    require_compatible_abi::<abi::ChatCommandInvocation>("ChatCommandInvocation", invocation.abi)?;
    let parsed_args = if invocation.parsed_arg_count == 0 {
      Vec::new()
    } else {
      if invocation.parsed_args.is_null() {
        return Err(PluginError::NullPointer("parsed_args"));
      }
      unsafe { std::slice::from_raw_parts(invocation.parsed_args, invocation.parsed_arg_count) }
        .iter()
        .map(CommandArgumentValueRef::from_abi)
        .collect::<Result<Vec<_>, _>>()?
    };
    Ok(Self {
      session_id: invocation.session_id,
      user_id: invocation.user_id,
      text_channel_id: invocation.text_channel_id,
      caller_role: invocation.caller_role,
      command_name: required_cstr(invocation.command_name, "command_name")?,
      args: optional_cstr(invocation.args)?,
      raw_text: required_cstr(invocation.raw_text, "raw_text")?,
      parsed_args,
    })
  }

  pub fn arg(&self, name: &str) -> Option<&CommandArgumentValueRef<'a>> {
    self.parsed_args.iter().find(|arg| arg.name == name)
  }
}

#[derive(Debug, Clone, Copy)]
pub struct CommandQueryRequestRef<'a> {
  pub session_id: SessionId,
  pub user_id: UserId,
  pub text_channel_id: ChannelId,
  pub caller_role: u8,
  pub request_id: u64,
  pub command_name: &'a str,
  pub argument_name: &'a str,
  pub query: &'a str,
  pub cursor_pos: u16,
}

impl<'a> CommandQueryRequestRef<'a> {
  /// Converts the ABI callback pointer into borrowed Rust strings.
  ///
  /// # Safety
  ///
  /// `request` must point to a valid ABI query request whose string pointers
  /// are valid for the duration of `on_chat_command_query`.
  pub unsafe fn from_raw(request: *const abi::CommandQueryRequest) -> Result<Self, PluginError> {
    if request.is_null() {
      return Err(PluginError::NullPointer("request"));
    }

    let request = unsafe { &*request };
    require_compatible_abi::<abi::CommandQueryRequest>("CommandQueryRequest", request.abi)?;
    Ok(Self {
      session_id: request.session_id,
      user_id: request.user_id,
      text_channel_id: request.text_channel_id,
      caller_role: request.caller_role,
      request_id: request.request_id,
      command_name: required_cstr(request.command_name, "query.command_name")?,
      argument_name: required_cstr(request.argument_name, "query.argument_name")?,
      query: required_cstr(request.query, "query.query")?,
      cursor_pos: request.cursor_pos,
    })
  }
}

#[derive(Debug, Clone, Copy)]
pub struct CommandArgumentValueRef<'a> {
  pub name: &'a str,
  pub type_: abi::CommandArgType,
  pub present: bool,
  pub i64_value: i64,
  pub u64_value: u64,
  pub f64_value: f64,
  pub bool_value: bool,
  pub string_value: &'a str,
}

impl<'a> CommandArgumentValueRef<'a> {
  fn from_abi(value: &abi::CommandArgumentValue) -> Result<Self, PluginError> {
    require_compatible_abi::<abi::CommandArgumentValue>("CommandArgumentValue", value.abi)?;
    Ok(Self {
      name: required_cstr(value.name, "argument.name")?,
      type_: abi::CommandArgType::from_u8(value.type_).ok_or(PluginError::UnknownCommandArgType(value.type_))?,
      present: value.present != 0,
      i64_value: value.i64_value,
      u64_value: value.u64_value,
      f64_value: value.f64_value,
      bool_value: value.bool_value != 0,
      string_value: optional_cstr(value.string_value)?,
    })
  }
}

#[derive(Debug, Clone, Copy)]
pub struct ChatMessageRef<'a> {
  pub session_id: SessionId,
  pub author_user_id: UserId,
  pub text_channel_id: ChannelId,
  pub author_name: &'a str,
  pub text: &'a str,
  pub attachment_count: u8,
}

impl<'a> ChatMessageRef<'a> {
  /// Converts the ABI callback pointer into borrowed Rust strings.
  ///
  /// # Safety
  ///
  /// `message` must point to a valid ABI chat message whose string pointers are
  /// valid for the returned lifetime. The server guarantees this for the
  /// duration of `on_chat_message`.
  pub unsafe fn from_raw(message: *const abi::ChatMessage) -> Result<Self, PluginError> {
    if message.is_null() {
      return Err(PluginError::NullPointer("message"));
    }

    let message = unsafe { &*message };
    require_compatible_abi::<abi::ChatMessage>("ChatMessage", message.abi)?;
    Ok(Self {
      session_id: message.session_id,
      author_user_id: message.author_user_id,
      text_channel_id: message.text_channel_id,
      author_name: required_cstr(message.author_name, "author_name")?,
      text: required_cstr(message.text, "text")?,
      attachment_count: message.attachment_count,
    })
  }
}

thread_local! {
  static CHAT_DECISION_STRINGS: RefCell<Vec<CString>> = const { RefCell::new(Vec::new()) };
  static COMMAND_QUERY_RESPONSE_STORAGE: RefCell<CommandQueryResponseStorage> = RefCell::new(CommandQueryResponseStorage::default());
}

#[derive(Default)]
struct CommandQueryResponseStorage {
  strings: Vec<CString>,
  results: Vec<abi::CommandQueryResult>,
}

impl CommandQueryResponseStorage {
  fn clear(&mut self) {
    self.strings.clear();
    self.results.clear();
  }

  fn push_string(&mut self, value: &str) -> *const std::ffi::c_char {
    let value = CString::new(value).unwrap_or_else(|_| CString::new(value.replace('\0', " ")).expect("nul sanitized"));
    self.strings.push(value);
    self.strings.last().expect("stored command query string").as_ptr()
  }
}

/// Writes a safe Rust chat decision into the ABI output slot.
///
/// # Safety
///
/// `decision` must be null or point to a valid writable `abi::ChatDecision`
/// provided by the server during `on_chat_message`.
pub unsafe fn write_chat_decision(decision: *mut abi::ChatDecision, value: ChatDecision) {
  if decision.is_null() {
    return;
  }

  CHAT_DECISION_STRINGS.with(|strings| {
    let mut strings = strings.borrow_mut();
    strings.clear();

    let mut abi_decision = abi::ChatDecision::continue_();
    match value {
      ChatDecision::Continue => {}
      ChatDecision::Reject { reason } => {
        if let Ok(reason) = CString::new(reason) {
          strings.push(reason);
          abi_decision.code = abi::ChatDecisionCode::Reject as u8;
          abi_decision.rejection_reason = strings.last().expect("stored rejection reason").as_ptr();
        }
      }
      ChatDecision::ReplaceText { text } => {
        if let Ok(text) = CString::new(text) {
          strings.push(text);
          abi_decision.code = abi::ChatDecisionCode::ReplaceText as u8;
          abi_decision.replacement_text = strings.last().expect("stored replacement text").as_ptr();
        }
      }
    }

    unsafe {
      *decision = abi_decision;
    }
  });
}

/// Writes a safe Rust command query response into the ABI output slot.
///
/// # Safety
///
/// `response` must be null or point to a valid writable
/// `abi::CommandQueryResponse` provided by the server during
/// `on_chat_command_query`.
pub unsafe fn write_command_query_response(response: *mut abi::CommandQueryResponse, value: CommandQueryResponse) {
  if response.is_null() {
    return;
  }

  with_abi_command_query_response(value, |abi_response| {
    unsafe {
      *response = *abi_response;
    }
    Ok(())
  })
  .ok();
}

fn with_abi_command_query_response<T>(
  value: CommandQueryResponse,
  callback: impl FnOnce(*const abi::CommandQueryResponse) -> Result<T, PluginError>,
) -> Result<T, PluginError> {
  COMMAND_QUERY_RESPONSE_STORAGE.with(|storage| {
    let mut storage = storage.borrow_mut();
    storage.clear();

    for result in &value.results {
      let id = storage.push_string(&result.id);
      let title = storage.push_string(&result.title);
      let subtitle = storage.push_string(&result.subtitle);
      let result_value = storage.push_string(&result.value);
      let kind = storage.push_string(&result.kind);
      let thumbnail_url = storage.push_string(&result.thumbnail_url);
      storage.results.push(abi::CommandQueryResult {
        abi: abi::AbiHeader::new::<abi::CommandQueryResult>(),
        id,
        title,
        subtitle,
        value: result_value,
        kind,
        duration_ms: result.duration_ms,
        thumbnail_url,
      });
    }

    let message = storage.push_string(&value.message);
    let abi_response = abi::CommandQueryResponse {
      abi: abi::AbiHeader::new::<abi::CommandQueryResponse>(),
      status: value.status as u8,
      message,
      results: if storage.results.is_empty() {
        std::ptr::null()
      } else {
        storage.results.as_ptr()
      },
      result_count: storage.results.len(),
    };

    callback(&abi_response)
  })
}

pub fn is_valid_command_name(name: &str) -> bool {
  !name.is_empty()
    && name.len() <= 64
    && name
      .bytes()
      .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub fn is_valid_command_argument_name(name: &str) -> bool {
  !name.is_empty() && name.len() <= 64 && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn require_compatible_abi<T>(ty: &'static str, abi: abi::AbiHeader) -> Result<(), PluginError> {
  if abi.is_compatible_with::<T>() {
    Ok(())
  } else {
    Err(PluginError::IncompatibleAbi {
      ty,
      expected_size: std::mem::size_of::<T>(),
      actual_size: abi.size,
      expected_major: abi::API_VERSION_MAJOR,
      actual_major: abi.api_major,
    })
  }
}

fn cstrings<'a>(values: impl Iterator<Item = &'a str>) -> Result<Vec<CString>, PluginError> {
  values
    .map(CString::new)
    .collect::<Result<Vec<_>, _>>()
    .map_err(Into::into)
}

fn required_cstr<'a>(value: *const std::ffi::c_char, field: &'static str) -> Result<&'a str, PluginError> {
  if value.is_null() {
    return Err(PluginError::NullPointer(field));
  }
  Ok(unsafe { CStr::from_ptr(value) }.to_str()?)
}

fn fixed_cstr(value: &[std::ffi::c_char]) -> Result<&str, PluginError> {
  let len = value.iter().position(|byte| *byte == 0).unwrap_or(value.len());
  let bytes = unsafe { std::slice::from_raw_parts(value.as_ptr().cast::<u8>(), len) };
  Ok(std::str::from_utf8(bytes)?)
}

fn optional_cstr<'a>(value: *const std::ffi::c_char) -> Result<&'a str, PluginError> {
  if value.is_null() {
    Ok("")
  } else {
    Ok(unsafe { CStr::from_ptr(value) }.to_str()?)
  }
}

#[cfg(test)]
mod tests {
  use std::ffi::c_void;

  use super::*;

  #[test]
  fn abi_headers_report_current_layout() {
    assert_eq!(abi::AbiHeader::new::<abi::Host>().api_major, 1);
    assert!(abi::AbiHeader::new::<abi::Host>().is_compatible_with::<abi::Host>());
  }

  #[test]
  fn permission_strings_round_trip() {
    for permission in IMPLEMENTED_PERMISSIONS.iter().chain(PLANNED_PERMISSIONS) {
      assert_eq!(PluginPermission::from_str(permission.as_str()), Some(*permission));
    }
  }

  #[test]
  fn command_name_validation_matches_plugin_rules() {
    assert!(is_valid_command_name("play"));
    assert!(is_valid_command_name("bot-ping_2"));
    assert!(!is_valid_command_name(""));
    assert!(!is_valid_command_name("/play"));
    assert!(!is_valid_command_name("play now"));
    assert!(!is_valid_command_name(&"x".repeat(65)));
  }

  #[test]
  fn invocation_ref_decodes_c_strings() {
    let command_name = CString::new("play").unwrap();
    let args = CString::new("https://example.test/song").unwrap();
    let raw_text = CString::new("/play https://example.test/song").unwrap();
    let invocation = abi::ChatCommandInvocation {
      abi: abi::AbiHeader::new::<abi::ChatCommandInvocation>(),
      session_id: 1,
      user_id: 2,
      text_channel_id: 3,
      caller_role: 3,
      command_name: command_name.as_ptr(),
      args: args.as_ptr(),
      raw_text: raw_text.as_ptr(),
      parsed_args: std::ptr::null(),
      parsed_arg_count: 0,
    };

    let decoded = unsafe { ChatCommandInvocationRef::from_raw(&invocation) }.unwrap();

    assert_eq!(decoded.command_name, "play");
    assert_eq!(decoded.caller_role, 3);
    assert_eq!(decoded.args, "https://example.test/song");
    assert_eq!(decoded.raw_text, "/play https://example.test/song");
  }

  #[test]
  fn host_ref_registers_rust_command_definitions() {
    unsafe extern "C" fn create_chat_commands(
      context: *mut c_void,
      commands: *const abi::CommandDefinition,
      command_count: usize,
    ) -> bool {
      let calls = unsafe { &mut *(context as *mut usize) };
      *calls += 1;
      assert_eq!(command_count, 1);
      let command = unsafe { &*commands };
      assert_eq!(unsafe { CStr::from_ptr(command.name) }.to_str().unwrap(), "play");
      assert_eq!(command.min_role, 2);
      true
    }

    let mut calls: usize = 0;
    let mut host = abi::Host::empty();
    host.context = (&mut calls as *mut usize).cast();
    host.create_chat_commands = Some(create_chat_commands);

    let host = unsafe { HostRef::from_raw(&host) }.unwrap();
    host
      .create_chat_commands(&[CommandDefinition::new("play", "Queue audio.", "/play {url:string}").with_min_role(2)])
      .unwrap();

    assert_eq!(calls, 1);
  }

  #[test]
  fn host_ref_maps_zero_voice_channel_to_none() {
    unsafe extern "C" fn user_voice_channel(
      context: *mut c_void,
      user_id: UserId,
      out_voice_channel_id: *mut ChannelId,
    ) -> bool {
      let calls = unsafe { &mut *(context as *mut usize) };
      *calls += 1;
      unsafe {
        *out_voice_channel_id = if user_id == 7 { 42 } else { 0 };
      }
      true
    }

    let mut calls: usize = 0;
    let mut host = abi::Host::empty();
    host.context = (&mut calls as *mut usize).cast();
    host.user_voice_channel = Some(user_voice_channel);

    let host = unsafe { HostRef::from_raw(&host) }.unwrap();

    assert_eq!(host.user_voice_channel(7).unwrap(), Some(42));
    assert_eq!(host.user_voice_channel(8).unwrap(), None);
    assert_eq!(calls, 2);
  }

  #[test]
  fn invocation_ref_decodes_typed_args() {
    let command_name = CString::new("bottypes").unwrap();
    let args = CString::new("true note").unwrap();
    let raw_text = CString::new("/bottypes true note").unwrap();
    let flag_name = CString::new("flag").unwrap();
    let note_name = CString::new("note").unwrap();
    let note_value = CString::new("hello world").unwrap();
    let parsed_args = [
      abi::CommandArgumentValue {
        abi: abi::AbiHeader::new::<abi::CommandArgumentValue>(),
        name: flag_name.as_ptr(),
        type_: abi::CommandArgType::Bool as u8,
        present: 1,
        i64_value: 0,
        u64_value: 0,
        f64_value: 0.0,
        bool_value: 1,
        string_value: std::ptr::null(),
      },
      abi::CommandArgumentValue {
        abi: abi::AbiHeader::new::<abi::CommandArgumentValue>(),
        name: note_name.as_ptr(),
        type_: abi::CommandArgType::String as u8,
        present: 1,
        i64_value: 0,
        u64_value: 0,
        f64_value: 0.0,
        bool_value: 0,
        string_value: note_value.as_ptr(),
      },
    ];
    let invocation = abi::ChatCommandInvocation {
      abi: abi::AbiHeader::new::<abi::ChatCommandInvocation>(),
      session_id: 1,
      user_id: 2,
      text_channel_id: 3,
      caller_role: 3,
      command_name: command_name.as_ptr(),
      args: args.as_ptr(),
      raw_text: raw_text.as_ptr(),
      parsed_args: parsed_args.as_ptr(),
      parsed_arg_count: parsed_args.len(),
    };

    let decoded = unsafe { ChatCommandInvocationRef::from_raw(&invocation) }.unwrap();

    assert_eq!(decoded.arg("flag").unwrap().type_, abi::CommandArgType::Bool);
    assert!(decoded.arg("flag").unwrap().bool_value);
    assert_eq!(decoded.arg("note").unwrap().string_value, "hello world");
  }

  #[test]
  fn host_ref_decodes_manifest_variables() {
    let key = CString::new("echo_prefix").unwrap();
    let value = CString::new("test").unwrap();
    let variables = [abi::PluginVariable {
      abi: abi::AbiHeader::new::<abi::PluginVariable>(),
      key: key.as_ptr(),
      value: value.as_ptr(),
    }];
    let mut host = abi::Host::empty();
    host.variables = variables.as_ptr();
    host.variable_count = variables.len();

    let host = unsafe { HostRef::from_raw(&host) }.unwrap();
    let variables = host.variables().unwrap();

    assert_eq!(variables[0].key, "echo_prefix");
    assert_eq!(variables[0].value, "test");
  }

  #[test]
  fn host_ref_lists_channels() {
    unsafe extern "C" fn list_voice_channels(
      _context: *mut c_void,
      out_channels: *mut abi::ChannelInfo,
      inout_count: *mut usize,
    ) -> bool {
      unsafe {
        *inout_count = 1;
        if out_channels.is_null() {
          return true;
        }
        let mut channel = abi::ChannelInfo {
          channel_id: 42,
          user_count: 3,
          max_users: 8,
          sort_order: 5,
          ..abi::ChannelInfo::default()
        };
        let name = b"General\0";
        for (index, byte) in name.iter().enumerate() {
          channel.name[index] = *byte as std::ffi::c_char;
        }
        *out_channels = channel;
      }
      true
    }

    let mut host = abi::Host::empty();
    host.list_voice_channels = Some(list_voice_channels);

    let host = unsafe { HostRef::from_raw(&host) }.unwrap();
    let channels = host.list_voice_channels().unwrap();

    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].channel_id, 42);
    assert_eq!(channels[0].name, "General");
  }
}
