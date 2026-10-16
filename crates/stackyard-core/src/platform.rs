//! The computer Stackyard runs on, as download sources name it.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Macos,
    Windows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X64,
    Arm64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Platform {
    pub os: Os,
    pub arch: Arch,
}

impl Platform {
    pub fn current() -> Self {
        let os = if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Macos
        } else {
            Os::Linux
        };
        let arch = if cfg!(target_arch = "aarch64") {
            Arch::Arm64
        } else {
            Arch::X64
        };
        Self { os, arch }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let arch = match self.arch {
            Arch::X64 => "x64",
            Arch::Arm64 => "ARM64",
        };
        write!(f, "{} {arch}", self.os)
    }
}

impl fmt::Display for Os {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Os::Linux => "Linux",
            Os::Macos => "macOS",
            Os::Windows => "Windows",
        })
    }
}

/// Whether this process runs as root (always false on Windows, where
/// administrator rights are asked for per action).
#[cfg(unix)]
pub fn running_as_root() -> bool {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() == 0 }
}

#[cfg(not(unix))]
pub fn running_as_root() -> bool {
    false
}

/// Keeps a console program from opening a console window on Windows. The
/// desktop app has no console of its own, so without this every helper it
/// runs (PowerShell, mysqld --initialize) flashes a window on screen.
pub fn hide_console(command: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = command;
}

/// Windows keeps a program running when the app that started it closes or
/// crashes, so a database server could outlive Stackyard and keep its files
/// and port. Each service joins a job that Windows ends together with this
/// process, however it ends.
#[cfg(windows)]
pub fn end_with_this_process(child: &std::process::Child) {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    // The handle stays open for the life of this process, on purpose: when
    // it closes, Windows ends every process in the job.
    static JOB: OnceLock<usize> = OnceLock::new();
    let job = *JOB.get_or_init(|| {
        // SAFETY: plain Win32 calls on a job this function owns; `info` is
        // a zeroed struct of the size passed.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return 0;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                std::mem::size_of_val(&info) as u32,
            );
            job as usize
        }
    });
    if job != 0 {
        // SAFETY: both handles are valid: the job above, and the child's
        // process handle, which `child` keeps open.
        unsafe {
            AssignProcessToJobObject(job as _, child.as_raw_handle() as _);
        }
    }
}

/// Ends copies of `program` still running from an earlier Stackyard (one
/// that was closed from Task Manager, or crashed): they hold the files and
/// ports a fresh start needs. Only that exact program file is matched.
pub fn stop_strays(program: &std::path::Path) {
    #[cfg(windows)]
    {
        // Windows reports `...\bin\mysqld.exe` where Stackyard has
        // `...\bin/mysqld.exe`, so both sides are made full paths first.
        let program: std::path::PathBuf = program.components().collect();
        let script = "$want = [IO.Path]::GetFullPath($env:STACKYARD_PROGRAM); \
            $p = Get-Process | Where-Object { $_.Path -and \
            [string]::Equals([IO.Path]::GetFullPath($_.Path), $want, [StringComparison]::OrdinalIgnoreCase) }; \
            if ($p) { $p | Stop-Process -Force -ErrorAction SilentlyContinue; \
            $p | Wait-Process -Timeout 10 -ErrorAction SilentlyContinue }";
        let _ = crate::system::powershell_command(script)
            .env("STACKYARD_PROGRAM", program)
            .status();
    }
    #[cfg(not(windows))]
    let _ = program;
}

#[cfg(all(test, windows))]
mod windows_tests {
    /// A copy left running by an earlier Stackyard is ended before a start.
    #[test]
    fn stops_a_stray_copy_of_a_program() {
        let dir = crate::testutil::tempdir();
        // Written the way services name their programs (`bin/mysqld.exe`),
        // which Windows reports with a backslash.
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let program = dir.join("bin/ping.exe");
        std::fs::copy(r"C:\Windows\System32\PING.EXE", &program).unwrap();
        let mut stray = std::process::Command::new(&program)
            .args(["-n", "60", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        super::stop_strays(&program);
        assert!(stray.try_wait().unwrap().is_some(), "still running");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
