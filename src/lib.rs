#![warn(missing_docs)]

//! # To query RDC sessions (connections)

windows::include_bindings!();

use anyhow::{anyhow, Result};
use log::{info, trace};
use std::{ffi::c_void, mem, slice};
use winsafe::WString;
use Windows::Win32::{
    Foundation::{HANDLE, PWSTR},
    System::{
        Diagnostics::Debug::GetLastError,
        RemoteDesktop::{
            WTSClientInfo, WTSCloseServer, WTSEnumerateSessionsW, WTSFreeMemory, WTSOpenServerW,
            WTSQuerySessionInformationW, WTSCLIENTW, WTS_SESSION_INFOW,
        },
        SystemInformation::{GetComputerNameExW, COMPUTER_NAME_FORMAT},
    },
};

/// Remote Server
pub struct RemoteServer {
    server_handle: HANDLE,
    /// Vector of sessions info
    sessions_list: Vec<RemoteDesktopSessionInfo>,
}

impl Drop for RemoteServer {
    fn drop(&mut self) {
        unsafe { WTSCloseServer(self.server_handle) };
    }
}

#[derive(Debug)]
/// Session Info
pub struct RemoteDesktopSessionInfo {
    session_id: u32,
    state: RemoteDesktopSessionState,
    client_info: ClientInfo,
}

impl<'a> Iterator for SessionInfoIter<'a> {
    type Item = &'a RemoteDesktopSessionInfo;
    fn next(&mut self) -> Option<Self::Item> {
        self.internal.iter().next()
    }
}

/// Session info iterator
pub struct SessionInfoIter<'a> {
    internal: &'a Vec<RemoteDesktopSessionInfo>,
}

#[derive(Debug)]
/// Client Info
pub(crate) struct ClientInfo {
    /// Connected user-name
    pub user: String,
    /// Connected client's NetBIOS name
    pub client: String,
    /// address of connected client
    pub address: (u32, [u16; 31]),
}

#[derive(Debug, PartialEq)]
/// Session state
pub enum RemoteDesktopSessionState {
    /// A user is logged on to the WinStation. This state occurs when a user is signed in and actively connected to the device.
    Active,
    /// The WinStation is connected to the client.
    Connected,
    /// The WinStation is in the process of connecting to the client.
    ConnectQuery,
    /// The WinStation is shadowing another WinStation.
    Shadow,
    /// The WinStation is active but the client is disconnected. This state occurs when a user is signed in but not actively connected to the device, such as when the user has chosen to exit to the lock screen.
    Disconnected,
    /// The WinStation is waiting for a client to connect.
    Idle,
    /// The WinStation is listening for a connection. A listener session waits for requests for new client connections. No user is logged on a listener session. A listener session cannot be reset, shadowed, or changed to a regular client session.
    Listen,
    /// The WinStation is being reset.
    Reset,
    /// The WinStation is down due to an error.
    Down,
    /// The WinStation is initializing.
    Init,
}

impl RemoteDesktopSessionState {
    fn get_variant(id: i32) -> Self {
        match id {
            0 => Self::Active,
            1 => Self::Connected,
            2 => Self::ConnectQuery,
            3 => Self::Shadow,
            4 => Self::Disconnected,
            5 => Self::Idle,
            6 => Self::Listen,
            7 => Self::Reset,
            8 => Self::Down,
            9 => Self::Init,
            _ => unreachable!(),
        }
    }
}

impl RemoteServer {
    /// Create RemoteServer connection for further queries
    pub fn new<S: Into<String>>(server_name: S) -> Result<Self> {
        let server_name = server_name.into();
        info!("Host-name: {}", server_name);
        let mut server_name = WString::from_str(&server_name);
        let server_handle = unsafe { WTSOpenServerW(PWSTR(server_name.as_mut_ptr())) };
        trace!("server handle: {:?}", server_handle);
        Ok(Self {
            server_handle,
            sessions_list: Vec::new(),
        })
    }

    /// Fetch information from connected server
    pub fn update_info(&mut self) -> Result<()> {
        info!("update requested!");
        let mut sessions: *mut WTS_SESSION_INFOW =
            unsafe { mem::MaybeUninit::uninit().assume_init() };
        let mut session_count = 0;
        let mut sessions_v: Vec<RemoteDesktopSessionInfo> = Vec::new();
        match unsafe {
            WTSEnumerateSessionsW(self.server_handle, 0, 1, &mut sessions, &mut session_count)
        }
        .0
        {
            0 => {
                let error = unsafe { GetLastError() };
                Err(anyhow!(
                    "couldn't read remote-desktop sessions info. error-code: {:?}",
                    error
                ))
            }
            _ => {
                info!("session count is: {}", session_count);
                let sessions_list =
                    unsafe { slice::from_raw_parts(sessions, session_count as usize) };
                for ss_ptr in sessions_list {
                    let ss = *ss_ptr;
                    sessions_v.push(RemoteDesktopSessionInfo {
                        session_id: ss.SessionId,
                        state: RemoteDesktopSessionState::get_variant(ss.State.0),
                        client_info: self.fetch_client_info(ss.SessionId)?,
                    });
                }
                unsafe { WTSFreeMemory(sessions as *mut c_void) };
                self.sessions_list = sessions_v;
                Ok(())
            }
        }
    }

    fn fetch_client_info(&self, session_id: u32) -> Result<ClientInfo> {
        let mut buffer_ptr = PWSTR::default();
        let mut byte_count = 0;
        match unsafe {
            WTSQuerySessionInformationW(
                self.server_handle,
                session_id,
                WTSClientInfo,
                &mut buffer_ptr,
                &mut byte_count,
            )
        }
        .0
        {
            0 => {
                let error = unsafe { GetLastError() };
                Err(anyhow!("couldn't read user-name. error-code: {:?}", error))
            }
            _ => {
                let client_info_ptr =
                    unsafe { mem::transmute::<*mut u16, *mut WTSCLIENTW>(buffer_ptr.0) };
                let client_info = unsafe { *client_info_ptr };
                trace!(
                    "client-info of session-id: {} is {:?}",
                    session_id,
                    client_info
                );
                unsafe { WTSFreeMemory(buffer_ptr.0 as *mut c_void) };
                let user =
                    WString::from_wchars_slice(&client_info.UserName[..]).to_string_checked()?;
                let client =
                    WString::from_wchars_slice(&client_info.ClientName[..]).to_string_checked()?;
                Ok(ClientInfo {
                    user,
                    client,
                    address: (client_info.ClientAddressFamily, client_info.ClientAddress),
                })
            }
        }
    }

    /// Returns iterator to go through all connections
    pub fn iter(&self) -> SessionInfoIter {
        SessionInfoIter {
            internal: &self.sessions_list,
        }
    }
}

/// Get host-name of current windows machine
pub fn get_host_name() -> Result<String> {
    let mut host_name_buffer = [0_u16; 256];
    let buffer_ptr = PWSTR(host_name_buffer.as_mut_ptr());
    let mut size: u32 = 256;
    match unsafe { GetComputerNameExW(COMPUTER_NAME_FORMAT(0), buffer_ptr, &mut size) }.0 {
        0 => {
            let error = unsafe { GetLastError() };
            Err(anyhow!("couldn't read host-name. error-code: {:?}", error))
        }
        _ => {
            let name = String::from_utf16(&host_name_buffer)?;
            Ok(name)
        }
    }
}
