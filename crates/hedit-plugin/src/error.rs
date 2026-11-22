//! Fixed failures shared by the worker protocol and editor integration.
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
#[repr(u8)]
pub enum Error {
    Sandbox = 1,
    Request,
    Initialization,
    Syntax,
    EntryPoint,
    Execution,
    Instructions,
    Memory,
    ReturnType,
    ReturnEncoding,
    ReturnSize,
    Worker,
    Deadline,
    Cancelled,
    Unavailable,
    Startup,
    Stopped,
    EditCount,
    SdkTransaction,
    SdkType,
    SdkLimit,
    ServiceBusy,
    ServiceResult,
    ServiceLimit,
}

impl Error {
    pub fn is_limit(self) -> bool {
        matches!(
            self,
            Self::Request
                | Self::Instructions
                | Self::Memory
                | Self::ReturnSize
                | Self::Deadline
                | Self::SdkLimit
                | Self::ServiceLimit
        )
    }

    #[cfg(any(unix, windows))]
    pub(crate) fn from_worker_code(code: u8) -> Self {
        match code {
            1 => Self::Sandbox,
            2 => Self::Request,
            3 => Self::Initialization,
            4 => Self::Syntax,
            5 => Self::EntryPoint,
            6 => Self::Execution,
            7 => Self::Instructions,
            8 => Self::Memory,
            9 => Self::ReturnType,
            10 => Self::ReturnEncoding,
            11 => Self::ReturnSize,
            _ => Self::Worker,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Sandbox => "The OS could not isolate the Lua worker.",
            Self::Request => "The Lua plugin input is invalid or exceeds its limits.",
            Self::Initialization => "The Lua runtime could not start.",
            Self::Syntax => "The plugin has a Lua syntax error.",
            Self::EntryPoint => "The Lua script must return a function.",
            Self::Execution => "The Lua plugin raised an error.",
            Self::Instructions => "The Lua plugin reached its instruction limit.",
            Self::Memory => "The Lua plugin reached its memory limit.",
            Self::ReturnType => "The Lua plugin must return a string.",
            Self::ReturnEncoding => "The plugin replacement must be UTF-8 text without NUL bytes.",
            Self::ReturnSize => "The Lua plugin replacement exceeds 256 KiB.",
            Self::Worker => "The Lua worker stopped or returned an invalid response.",
            Self::Deadline => "The Lua plugin exceeded its time limit.",
            Self::Cancelled => "The Lua plugin was cancelled.",
            Self::Unavailable => "Lua plugins are unavailable on this platform.",
            Self::Startup => "The Lua worker could not start.",
            Self::Stopped => "The Lua worker stopped.",
            Self::EditCount => "The Lua worker returned the wrong number of edits.",
            Self::SdkTransaction => {
                "The editing SDK proposed an invalid or unauthorized transaction."
            }
            Self::SdkType => "The editing SDK must return a valid transaction table.",
            Self::ServiceResult => "The provider returned an invalid or unauthorized result.",
            Self::ServiceLimit => "The provider exceeded its documented result or resource limits.",
            Self::ServiceBusy => "The editor's concurrent provider limit is reached.",
            Self::SdkLimit => "The editing SDK exceeded its documented result or resource limits.",
        })
    }
}

impl std::error::Error for Error {}
