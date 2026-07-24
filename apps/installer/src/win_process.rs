// Windows 进程检测与终止
//   同用户进程无需管理员权限

use std::thread;
use std::time::Duration;

/// 已知的 DeepX 进程名
const DEEPX_PROCESSES: &[&str] = &["DeepX.exe", "deepx-daemon.exe"];

/// 进程信息
#[derive(Clone, Debug)]
pub struct ProcInfo {
    pub pid: u32,
    pub name: String,
    pub closed: bool,
}

// ============================================================
// 进程检测
// ============================================================

pub fn find_deepx_processes() -> Vec<ProcInfo> {
    find_via_toolhelp().unwrap_or_else(|| find_via_tasklist())
}

/// 通过 Toolhelp 快照检测（主要方法）
fn find_via_toolhelp() -> Option<Vec<ProcInfo>> {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW,
            PROCESSENTRY32W, TH32CS_SNAPPROCESS,
        };
        use windows::Win32::Foundation::CloseHandle;

        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut pe = PROCESSENTRY32W::default();
        pe.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        let mut result = Vec::new();
        if Process32FirstW(snap, &mut pe).is_ok() {
            loop {
                let len = pe.szExeFile.iter().position(|&c| c == 0).unwrap_or(pe.szExeFile.len());
                let name = String::from_utf16_lossy(&pe.szExeFile[..len]);
                if DEEPX_PROCESSES.iter().any(|p| p.eq_ignore_ascii_case(&name)) {
                    result.push(ProcInfo { pid: pe.th32ProcessID, name, closed: false });
                }
                if Process32NextW(snap, &mut pe).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        Some(result)
    }
    #[cfg(not(windows))]
    { None }
}

/// fallback：tasklist
fn find_via_tasklist() -> Vec<ProcInfo> {
    let mut result = Vec::new();
    for name in DEEPX_PROCESSES {
        if let Ok(out) = std::process::Command::new("tasklist")
            .args(["/fo", "csv", "/nh", "/fi", &format!("imagename eq {}", name)])
            .output()
        {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let parts: Vec<&str> = line.split(',').map(|s| s.trim_matches('"').trim()).collect();
                if parts.len() >= 2 {
                    if let Ok(pid) = parts[1].parse() {
                        result.push(ProcInfo { pid, name: name.to_string(), closed: false });
                    }
                }
            }
        }
    }
    result
}

// ============================================================
// 进程关闭
// ============================================================

/// 优雅关闭：先 taskkill（不带 /f），给进程 2 秒自行清理
pub fn graceful_close(procs: &mut [ProcInfo]) {
    for p in procs.iter_mut() {
        // taskkill 不带 /f = 发送 WM_CLOSE
        let _ = std::process::Command::new("taskkill")
            .args(["/pid", &p.pid.to_string()])
            .output();
    }
    // 等 2 秒让进程自行退出
    thread::sleep(Duration::from_secs(2));
    // 刷新状态
    for p in procs.iter_mut() {
        p.closed = !is_process_running(p.pid);
    }
}

/// 强制终止
pub fn force_terminate(pid: u32) -> bool {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
        use windows::Win32::Foundation::CloseHandle;
        if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) {
            let ok = TerminateProcess(h, 0).is_ok();
            let _ = CloseHandle(h);
            return ok;
        }
    }
    // fallback: taskkill /f
    let _ = std::process::Command::new("taskkill")
        .args(["/f", "/pid", &pid.to_string()])
        .output();
    false
}

/// 等待进程退出
pub fn wait_for_exit(procs: &[ProcInfo], timeout_secs: u64) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    let pids: Vec<u32> = procs.iter().map(|p| p.pid).collect();
    while std::time::Instant::now() < deadline {
        if pids.iter().all(|&pid| !is_process_running(pid)) {
            return true;
        }
        thread::sleep(Duration::from_millis(300));
    }
    false
}

/// 检查进程是否仍在运行
pub fn is_alive(pid: u32) -> bool {
    is_process_running(pid)
}

fn is_process_running(pid: u32) -> bool {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, GetExitCodeProcess};
        use windows::Win32::Foundation::CloseHandle;
        if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut code: u32 = 259; // STILL_ACTIVE
            let _ = GetExitCodeProcess(h, &mut code);
            let _ = CloseHandle(h);
            return code == 259;
        }
        false
    }
    #[cfg(not(windows))]
    { let _ = pid; false }
}
