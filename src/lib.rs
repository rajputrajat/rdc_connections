#![deny(missing_docs)]

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

pub struct RemoteServer {
    server_handle: HANDLE,
    pub sessions_list: Option<Vec<RemoteDesktopSessionInfo>>,
}

impl Drop for RemoteServer {
    fn drop(&mut self) {
        unsafe { WTSCloseServer(self.server_handle) };
    }
}

#[derive(Debug)]
pub struct RemoteDesktopSessionInfo {
    session_id: u32,
    state: RemoteDesktopSessionState,
    client_info: ClientInfo,
}

#[derive(Debug)]
pub struct ClientInfo {
    pub user: String,
    pub client: String,
    pub address: (u32, [u16; 31]),
}

#[derive(Debug, PartialEq)]
pub enum RemoteDesktopSessionState {
    Active,
    Connected,
    ConnectQuery,
    Shadow,
    Disconnected,
    Idle,
    Listen,
    Reset,
    Down,
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
    pub fn new<S: Into<String>>(server_name: S) -> Result<Self> {
        let server_name = server_name.into();
        info!("Host-name: {}", server_name);
        let mut server_name = WString::from_str(&server_name);
        let server_handle = unsafe { WTSOpenServerW(PWSTR(server_name.as_mut_ptr())) };
        trace!("server handle: {:?}", server_handle);
        Ok(Self {
            server_handle,
            sessions_list: None,
        })
    }

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
                self.sessions_list = Some(sessions_v);
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
}

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
