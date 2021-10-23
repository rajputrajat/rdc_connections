fn main() {
    windows::build! {
        Windows::Win32::{
            Foundation::{PWSTR, HANDLE},
            System::{
                RemoteDesktop::{
                    WTSEnumerateSessionsW, WTS_SESSION_INFOW,
                    WTSOpenServerW, WTSCloseServer,
                    WTSFreeMemory,
                    WTSQuerySessionInformationW, WTS_INFO_CLASS, WTSCLIENTW
                },
                SystemInformation::{GetComputerNameExW, COMPUTER_NAME_FORMAT},
                WindowsProgramming::GetUserNameW,
                Diagnostics::Debug::GetLastError
            },
        }
    };
}
