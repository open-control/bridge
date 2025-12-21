//! UAC elevation for Windows
//!
//! Provides functions to check and request administrator privileges.

use anyhow::Result;

/// Check if the current process is running with elevated privileges
#[cfg(windows)]
pub fn is_elevated() -> bool {
    use std::mem;
    use std::ptr;

    #[link(name = "advapi32")]
    extern "system" {
        fn OpenProcessToken(
            ProcessHandle: *mut std::ffi::c_void,
            DesiredAccess: u32,
            TokenHandle: *mut *mut std::ffi::c_void,
        ) -> i32;
        fn GetTokenInformation(
            TokenHandle: *mut std::ffi::c_void,
            TokenInformationClass: u32,
            TokenInformation: *mut std::ffi::c_void,
            TokenInformationLength: u32,
            ReturnLength: *mut u32,
        ) -> i32;
        fn CloseHandle(hObject: *mut std::ffi::c_void) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
    }

    const TOKEN_QUERY: u32 = 0x0008;
    const TOKEN_ELEVATION: u32 = 20;

    #[repr(C)]
    struct TokenElevation {
        token_is_elevated: u32,
    }

    unsafe {
        let mut token: *mut std::ffi::c_void = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }

        let mut elevation: TokenElevation = mem::zeroed();
        let mut size: u32 = 0;
        let result = GetTokenInformation(
            token,
            TOKEN_ELEVATION,
            &mut elevation as *mut _ as *mut std::ffi::c_void,
            mem::size_of::<TokenElevation>() as u32,
            &mut size,
        );
        CloseHandle(token);

        result != 0 && elevation.token_is_elevated != 0
    }
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    // On Unix, check if running as root
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// Restart the current process with elevated privileges (UAC prompt)
#[cfg(windows)]
pub fn restart_elevated() -> Result<()> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            hwnd: *mut std::ffi::c_void,
            lpOperation: *const u16,
            lpFile: *const u16,
            lpParameters: *const u16,
            lpDirectory: *const u16,
            nShowCmd: i32,
        ) -> isize;
    }

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let exe = std::env::current_exe()?;
    let exe_wide = to_wide(&exe.to_string_lossy());
    let verb = to_wide("runas");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args_str = args.join(" ");
    let args_wide = to_wide(&args_str);

    const SW_SHOWNORMAL: i32 = 1;

    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            verb.as_ptr(),
            exe_wide.as_ptr(),
            args_wide.as_ptr(),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    };

    if result > 32 {
        // Success - exit current process
        std::process::exit(0);
    } else {
        anyhow::bail!("Failed to restart with elevation (error code: {})", result);
    }
}

#[cfg(not(windows))]
pub fn restart_elevated() -> Result<()> {
    anyhow::bail!("Elevation not supported on this platform. Run with sudo.")
}

/// Check if elevation is required for a specific operation
pub fn requires_elevation(operation: &str) -> bool {
    match operation {
        "install" | "uninstall" => !is_elevated(),
        _ => false,
    }
}

/// Run a specific action with elevated privileges (UAC prompt)
/// Unlike restart_elevated, this does NOT exit the current process.
/// The elevated process runs independently and the caller continues.
#[cfg(windows)]
pub fn run_elevated_action(action: &str) -> Result<()> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            hwnd: *mut std::ffi::c_void,
            lpOperation: *const u16,
            lpFile: *const u16,
            lpParameters: *const u16,
            lpDirectory: *const u16,
            nShowCmd: i32,
        ) -> isize;
    }

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let exe = std::env::current_exe()?;
    let exe_wide = to_wide(&exe.to_string_lossy());
    let verb = to_wide("runas");
    let args_wide = to_wide(action);

    const SW_SHOWNORMAL: i32 = 1;

    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            verb.as_ptr(),
            exe_wide.as_ptr(),
            args_wide.as_ptr(),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    };

    if result > 32 {
        Ok(())
    } else {
        anyhow::bail!("Failed to run elevated action (error code: {})", result);
    }
}

#[cfg(not(windows))]
pub fn run_elevated_action(_action: &str) -> Result<()> {
    anyhow::bail!("Elevation not supported on this platform. Run with sudo.")
}
