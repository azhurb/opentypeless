//! Foreground coding-CLI detection.
//!
//! Some terminal-hosted coding CLIs (Claude Code, Codex, Gemini CLI) collapse
//! or mangle large clipboard pastes. To chunk the paste for them, the output
//! stage must know one is running in the *focused* terminal. The window title
//! is an unreliable signal for this: an IDE's integrated terminal reports the
//! project/file name, not the CLI, so a title match never fires there.
//!
//! Instead we resolve the CLI from the process table: find a process whose
//! name matches a known CLI and that is a descendant of the focused
//! application. A descendant ⇒ the CLI is running inside the focused
//! terminal/IDE (`High` confidence). A match found elsewhere ⇒ `Low`.
//!
//! Process enumeration uses libproc (`proc_listpids` / `proc_pidinfo`), which
//! needs no special permission for the calling user's own processes.

/// A known coding CLI we chunk pastes for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliKind {
    Claude,
    Codex,
    Gemini,
}

/// How sure we are that the CLI is the focused paste target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    /// The CLI process is a descendant of the focused application.
    High,
    /// A matching CLI process exists, but not under the focused application.
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetectedCli {
    pub kind: CliKind,
    pub confidence: Confidence,
}

/// A single process in a snapshot: pid, parent pid, and executable basename.
#[derive(Debug, Clone)]
pub struct Proc {
    pub pid: i32,
    pub ppid: i32,
    pub name: String,
}

/// Map a process name (the executable basename, as the OS reports it) to a
/// known coding CLI. The match is exact and case-sensitive: these CLIs ship as
/// binaries literally named `claude` / `codex` / `gemini`. Exactness keeps us
/// from matching look-alikes such as the `GeminiAppLauncher` desktop app.
pub fn cli_kind_from_name(name: &str) -> Option<CliKind> {
    match name {
        "claude" => Some(CliKind::Claude),
        "codex" => Some(CliKind::Codex),
        "gemini" => Some(CliKind::Gemini),
        _ => None,
    }
}

/// Resolve which coding CLI (if any) is running, given a process snapshot and
/// the focused application's pid. A matching CLI process that descends from
/// `frontmost_pid` is reported `High`; a match elsewhere is `Low`. `High`
/// wins over `Low`.
pub fn detect_from_processes(procs: &[Proc], frontmost_pid: i32) -> Option<DetectedCli> {
    let mut low: Option<CliKind> = None;
    for p in procs {
        let Some(kind) = cli_kind_from_name(&p.name) else {
            continue;
        };
        if is_descendant_of(procs, p.pid, frontmost_pid) {
            return Some(DetectedCli {
                kind,
                confidence: Confidence::High,
            });
        }
        low.get_or_insert(kind);
    }
    low.map(|kind| DetectedCli {
        kind,
        confidence: Confidence::Low,
    })
}

/// Whether `pid`'s parent chain passes through `ancestor`. Walks ppid links
/// upward, with a hop limit that guards against cycles in a malformed table.
/// pid 0/1 (kernel/launchd) are not meaningful ancestors — everything descends
/// from launchd — so an `ancestor` of 0 or 1 is never a match.
fn is_descendant_of(procs: &[Proc], pid: i32, ancestor: i32) -> bool {
    if ancestor <= 1 {
        return false;
    }
    let parent_of = |q: i32| procs.iter().find(|p| p.pid == q).map(|p| p.ppid);
    let mut cur = pid;
    for _ in 0..64 {
        let Some(pp) = parent_of(cur) else {
            return false;
        };
        if pp == ancestor {
            return true;
        }
        if pp <= 1 {
            return false;
        }
        cur = pp;
    }
    false
}

#[cfg(target_os = "macos")]
pub fn list_processes() -> Vec<Proc> {
    macos_proc::list()
}

#[cfg(not(target_os = "macos"))]
pub fn list_processes() -> Vec<Proc> {
    // Non-macOS platforms fall back to the window-title heuristic (arm B) in
    // the chunker; no process scan here yet.
    Vec::new()
}

/// libproc-backed process enumeration. `proc_listpids` lists all pids,
/// `proc_pidinfo(PROC_PIDT_SHORTBSDINFO)` yields each process's ppid, and
/// `sysctl(KERN_PROCARGS2)` yields argv[0] (the CLI's invocation name). All are
/// in libSystem and need no special permission for the calling user's own
/// processes. ~12 ms for ~950 processes; runs at paste time, off the hot path.
#[cfg(target_os = "macos")]
mod macos_proc {
    use std::ffi::c_void;

    use super::Proc;

    const PROC_ALL_PIDS: u32 = 1;
    const PROC_PIDT_SHORTBSDINFO: i32 = 13;
    const CTL_KERN: i32 = 1;
    const KERN_PROCARGS2: i32 = 49;

    // Layout of `struct proc_bsdshortinfo` from <sys/proc_info.h>. MAXCOMLEN is
    // 16. Total size is 64 bytes; `proc_pidinfo` returns that on success.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct ProcBsdShortInfo {
        pbsi_pid: u32,
        pbsi_ppid: u32,
        pbsi_pgid: u32,
        pbsi_status: u32,
        pbsi_comm: [u8; 16],
        pbsi_flags: u32,
        pbsi_uid: u32,
        pbsi_gid: u32,
        pbsi_ruid: u32,
        pbsi_rgid: u32,
        pbsi_svuid: u32,
        pbsi_svgid: u32,
        pbsi_rfu: u32,
    }

    extern "C" {
        fn proc_listpids(t: u32, typeinfo: u32, buffer: *mut c_void, buffersize: i32) -> i32;
        fn proc_pidinfo(
            pid: i32,
            flavor: i32,
            arg: u64,
            buffer: *mut c_void,
            buffersize: i32,
        ) -> i32;
        fn sysctl(
            name: *mut i32,
            namelen: u32,
            oldp: *mut c_void,
            oldlenp: *mut usize,
            newp: *mut c_void,
            newlen: usize,
        ) -> i32;
    }

    pub fn list() -> Vec<Proc> {
        let pid_size = std::mem::size_of::<u32>() as i32;
        unsafe {
            // Size query: how many bytes of pids are there?
            let bytes = proc_listpids(PROC_ALL_PIDS, 0, std::ptr::null_mut(), 0);
            if bytes <= 0 {
                return Vec::new();
            }
            // Over-allocate: processes may spawn between the two calls.
            let cap = (bytes / pid_size) as usize + 64;
            let mut pids: Vec<u32> = vec![0; cap];
            let filled = proc_listpids(
                PROC_ALL_PIDS,
                0,
                pids.as_mut_ptr() as *mut c_void,
                (cap * pid_size as usize) as i32,
            );
            if filled <= 0 {
                return Vec::new();
            }
            pids.truncate((filled / pid_size) as usize);

            let info_size = std::mem::size_of::<ProcBsdShortInfo>() as i32;
            let mut out = Vec::with_capacity(pids.len());
            for &pid in &pids {
                if pid == 0 {
                    continue;
                }
                let mut info: ProcBsdShortInfo = std::mem::zeroed();
                let ret = proc_pidinfo(
                    pid as i32,
                    PROC_PIDT_SHORTBSDINFO,
                    0,
                    &mut info as *mut _ as *mut c_void,
                    info_size,
                );
                // Short reads mean the process vanished or we lack access; skip.
                if ret != info_size {
                    continue;
                }
                // Identify the process by argv[0], not the kernel executable
                // name (`pbsi_comm`). Coding CLIs are invoked as `claude` /
                // `codex` / `gemini`, but their on-disk executable is often a
                // versioned or relocated binary (e.g. Claude Code runs
                // `~/.local/share/claude/versions/<ver>`), so `pbsi_comm` would
                // be the version string. argv[0] carries the real CLI name.
                // Fall back to `pbsi_comm` when argv[0] is unavailable (e.g.
                // other-user or kernel processes we can't read args for).
                let name = argv0_basename(pid as i32)
                    .unwrap_or_else(|| comm_to_string(&info.pbsi_comm));
                out.push(Proc {
                    pid: info.pbsi_pid as i32,
                    ppid: info.pbsi_ppid as i32,
                    name,
                });
            }
            out
        }
    }

    fn comm_to_string(comm: &[u8; 16]) -> String {
        let end = comm.iter().position(|&b| b == 0).unwrap_or(comm.len());
        String::from_utf8_lossy(&comm[..end]).into_owned()
    }

    /// Read the basename of argv[0] for `pid` via `sysctl(KERN_PROCARGS2)`.
    ///
    /// The buffer layout is `[argc: i32][exec_path\0][\0 padding][argv[0]\0]
    /// [argv[1]\0]…[env…]`. We skip argc and the executable path, then return
    /// the basename of the first argument string. Returns `None` if the args
    /// can't be read (insufficient privilege, kernel process, or a race).
    fn argv0_basename(pid: i32) -> Option<String> {
        unsafe {
            let mut mib = [CTL_KERN, KERN_PROCARGS2, pid];
            let mut size: usize = 0;
            if sysctl(
                mib.as_mut_ptr(),
                3,
                std::ptr::null_mut(),
                &mut size,
                std::ptr::null_mut(),
                0,
            ) != 0
                || size < 4
            {
                return None;
            }
            let mut buf = vec![0u8; size];
            if sysctl(
                mib.as_mut_ptr(),
                3,
                buf.as_mut_ptr() as *mut c_void,
                &mut size,
                std::ptr::null_mut(),
                0,
            ) != 0
            {
                return None;
            }
            buf.truncate(size);
            // Skip argc (first 4 bytes), then the NUL-terminated executable
            // path, then any NUL padding, to reach argv[0].
            let rest = buf.get(4..)?;
            let path_end = rest.iter().position(|&b| b == 0)?;
            let mut i = path_end;
            while i < rest.len() && rest[i] == 0 {
                i += 1;
            }
            let argv0_end = rest[i..]
                .iter()
                .position(|&b| b == 0)
                .map(|x| x + i)
                .unwrap_or(rest.len());
            let argv0 = &rest[i..argv0_end];
            if argv0.is_empty() {
                return None;
            }
            let s = String::from_utf8_lossy(argv0);
            Some(s.rsplit('/').next().unwrap_or(&s).to_string())
        }
    }
}

/// Detect the coding CLI running in the focused application's process tree.
/// Returns `None` when the pid is unusable, the process list can't be read, or
/// no known CLI is running.
pub fn detect_foreground_cli(frontmost_pid: i32) -> Option<DetectedCli> {
    if frontmost_pid <= 1 {
        return None;
    }
    let procs = list_processes();
    if procs.is_empty() {
        return None;
    }
    detect_from_processes(&procs, frontmost_pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: i32, ppid: i32, name: &str) -> Proc {
        Proc {
            pid,
            ppid,
            name: name.to_string(),
        }
    }

    #[test]
    fn matches_known_cli_binaries() {
        assert_eq!(cli_kind_from_name("claude"), Some(CliKind::Claude));
        assert_eq!(cli_kind_from_name("codex"), Some(CliKind::Codex));
        assert_eq!(cli_kind_from_name("gemini"), Some(CliKind::Gemini));
    }

    #[test]
    fn rejects_lookalikes_and_runtimes() {
        // The Gemini *desktop app* launcher must not be taken for the CLI.
        assert_eq!(cli_kind_from_name("GeminiAppLauncher"), None);
        // `comm` is truncated to 15 chars, so this is the form we'd actually see.
        assert_eq!(cli_kind_from_name("GeminiAppLaunch"), None);
        // A node-wrapped CLI shows up as the runtime, not the CLI name.
        assert_eq!(cli_kind_from_name("node"), None);
        // Case-sensitive: only the lowercase binary name matches.
        assert_eq!(cli_kind_from_name("Claude"), None);
        assert_eq!(cli_kind_from_name(""), None);
    }

    #[test]
    fn high_confidence_when_cli_descends_from_focused_app() {
        // Real topology observed on a dev machine:
        //   IntelliJ IDEA (53586) → /bin/zsh (54473) → claude (55503)
        let procs = vec![
            proc(53586, 1, "idea"),
            proc(54473, 53586, "zsh"),
            proc(55503, 54473, "claude"),
        ];
        assert_eq!(
            detect_from_processes(&procs, 53586),
            Some(DetectedCli {
                kind: CliKind::Claude,
                confidence: Confidence::High,
            })
        );
    }

    #[test]
    fn low_confidence_when_cli_runs_under_a_different_app() {
        // claude runs under some other terminal, not the focused app (9999).
        let procs = vec![
            proc(53586, 1, "idea"),
            proc(54473, 53586, "zsh"),
            proc(55503, 54473, "claude"),
        ];
        assert_eq!(
            detect_from_processes(&procs, 9999),
            Some(DetectedCli {
                kind: CliKind::Claude,
                confidence: Confidence::Low,
            })
        );
    }

    #[test]
    fn gemini_desktop_app_is_not_detected_as_cli() {
        // The Gemini.app launcher (ppid 1) must never be matched, even when a
        // terminal is focused.
        let procs = vec![
            proc(45965, 1, "GeminiAppLaunch"),
            proc(53586, 1, "idea"),
            proc(54473, 53586, "zsh"),
        ];
        assert_eq!(detect_from_processes(&procs, 53586), None);
    }

    #[test]
    fn none_when_no_cli_running() {
        let procs = vec![proc(53586, 1, "idea"), proc(54473, 53586, "zsh")];
        assert_eq!(detect_from_processes(&procs, 53586), None);
    }

    #[test]
    fn cycle_in_process_table_does_not_hang() {
        // Defensive: a pathological ppid cycle must terminate, not spin.
        let procs = vec![proc(10, 20, "claude"), proc(20, 10, "zsh")];
        // frontmost 999 is unrelated, so this is a Low match, but the point is
        // the walk terminates.
        assert_eq!(
            detect_from_processes(&procs, 999),
            Some(DetectedCli {
                kind: CliKind::Claude,
                confidence: Confidence::Low,
            })
        );
    }
}

#[cfg(all(test, target_os = "macos"))]
mod ffi_tests {
    use super::*;

    extern "C" {
        fn getppid() -> i32;
    }

    #[test]
    fn list_processes_includes_self_with_a_name() {
        let procs = list_processes();
        let me = std::process::id() as i32;
        let mine = procs.iter().find(|p| p.pid == me);
        assert!(mine.is_some(), "own pid {me} not found in process list");
        assert!(
            !mine.unwrap().name.is_empty(),
            "own process has empty name — FFI struct layout is likely wrong"
        );
    }

    // Ground-truth check that the `pbsi_ppid` offset is correct: the ppid we
    // read from the FFI for our own process must match libc `getppid()`. A
    // mismatch means the proc_bsdshortinfo struct layout is wrong and the
    // descendant walk would silently never match.
    #[test]
    fn reported_ppid_matches_getppid() {
        let procs = list_processes();
        let me = std::process::id() as i32;
        let mine = procs
            .iter()
            .find(|p| p.pid == me)
            .expect("own pid not in process list");
        let real = unsafe { getppid() };
        assert_eq!(
            mine.ppid, real,
            "FFI ppid {} != getppid() {} — proc_bsdshortinfo layout is wrong",
            mine.ppid, real
        );
    }

    // Regression: a coding CLI's on-disk executable may be a versioned or
    // relocated binary (Claude Code runs `.../claude/versions/<ver>`), so the
    // kernel executable name is NOT the CLI name. The process is still invoked
    // as argv[0]="claude" — that's the identity we must report. Spawn a child
    // whose argv[0] differs from its executable basename and require argv[0].
    #[test]
    fn name_is_argv0_not_executable_basename() {
        use std::os::unix::process::CommandExt;
        use std::process::Command;

        let mut child = Command::new("/bin/sleep")
            .arg0("claude-argv0-marker")
            .arg("30")
            .spawn()
            .expect("spawn /bin/sleep");
        let pid = child.id() as i32;
        // Let the exec land in the kernel argument table before we read it.
        std::thread::sleep(std::time::Duration::from_millis(80));

        let found = list_processes().into_iter().find(|p| p.pid == pid);

        let _ = child.kill();
        let _ = child.wait();

        let found = found.expect("spawned child not found in process list");
        assert_eq!(
            found.name, "claude-argv0-marker",
            "name must be argv[0] basename, not the executable basename"
        );
    }
}
