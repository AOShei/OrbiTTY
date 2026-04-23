use std::time::Duration;

const SHELLS: &[&str] = &[
    "bash", "zsh", "fish", "sh", "dash", "ksh", "csh", "tcsh",
    "nu", "nushell", "elvish", "ion", "xonsh", "pwsh",
];

/// Parse pgrp and tpgid from /proc/[pid]/stat.
fn parse_pgrp_tpgid(pid: i32) -> Option<(i32, i32)> {
    let data = std::fs::read_to_string(format!("/proc/{}/stat", pid)).ok()?;
    let pos = data.rfind(')')?;
    let rest = &data[pos + 2..];
    let fields: Vec<&str> = rest.split_whitespace().collect();
    if fields.len() <= 5 {
        return None;
    }
    let pgrp: i32 = fields[2].parse().ok()?;
    let tpgid: i32 = fields[5].parse().ok()?;
    Some((pgrp, tpgid))
}

/// Check whether the terminal is waiting for user input.
/// Handles nested shells (e.g. `su root` spawning a new bash).
pub(super) fn is_terminal_idle(shell_pid: i32) -> bool {
    let Some((pgrp, tpgid)) = parse_pgrp_tpgid(shell_pid) else {
        return true;
    };
    if pgrp == tpgid {
        return true; // Original shell is the foreground -> idle.
    }
    // Something else is foreground. If the fg leader is a shell at a
    // prompt (e.g. root bash from `su`), the terminal is still "idle".
    if tpgid > 0 {
        if let Ok(comm) = std::fs::read_to_string(format!("/proc/{}/comm", tpgid)) {
            let name = comm.trim();
            // Strip leading '-' (login shell convention, e.g. "-bash").
            let name = name.strip_prefix('-').unwrap_or(name);
            if SHELLS.contains(&name) {
                // The fg leader is a shell. Check that IT is the foreground
                // (it hasn't spawned a child command that took over).
                if let Some((fg_pgrp, fg_tpgid)) = parse_pgrp_tpgid(tpgid) {
                    return fg_pgrp == fg_tpgid;
                }
            }
        }
    }
    false
}

/// Full command line of the foreground process currently running in the
/// terminal, or None when the shell itself is at a prompt. Descends through
/// nested shells (e.g. `su -` spawning bash) to reach the actual command.
///
/// Reads `/proc/<pid>/cmdline` rather than `/proc/<pid>/comm` so the user
/// sees the real argv - `comm` is truncated to 15 chars and reflects the
/// thread name for runtimes that set one (Node.js -> "MainThread").
pub(super) fn foreground_command(shell_pid: i32) -> Option<String> {
    let (pgrp, tpgid) = parse_pgrp_tpgid(shell_pid)?;
    if tpgid <= 0 || pgrp == tpgid {
        return None;
    }
    // Use comm only for the shell-detection check (recurse through nested
    // shells). Comm is cheap and well-suited for that purpose.
    let comm = std::fs::read_to_string(format!("/proc/{}/comm", tpgid)).ok()?;
    let raw = comm.trim();
    let name = raw.strip_prefix('-').unwrap_or(raw);
    if SHELLS.contains(&name) {
        return foreground_command(tpgid);
    }
    // Real command: read argv from /proc/<pid>/cmdline (null-separated).
    let cmdline_bytes = std::fs::read(format!("/proc/{}/cmdline", tpgid)).ok()?;
    let parts: Vec<String> = cmdline_bytes
        .split(|&b| b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect();
    if parts.is_empty() {
        return Some(name.to_string());
    }
    Some(parts.join(" "))
}

/// Check whether the terminal's foreground process is running with root
/// privileges (euid == 0). Used to tint the header bar red.
pub(super) fn is_foreground_elevated(shell_pid: i32) -> bool {
    let Some((_pgrp, tpgid)) = parse_pgrp_tpgid(shell_pid) else {
        return false;
    };
    if tpgid <= 0 {
        return false;
    }
    let Ok(status) = std::fs::read_to_string(format!("/proc/{}/status", tpgid)) else {
        return false;
    };
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("Uid:") {
            let fields: Vec<&str> = rest.split_whitespace().collect();
            // Uid: real effective saved filesystem
            if let Some(euid_str) = fields.get(1) {
                return euid_str.parse::<u32>().unwrap_or(u32::MAX) == 0;
            }
        }
    }
    false
}

/// PID to use for CWD tracking. Prefers the terminal's foreground process
/// group leader (tpgid) over the original shell PID, so nested shells such
/// as a root bash opened via `sudo bash` report their own working directory
/// rather than the original shell's (unchanged) home directory.
/// Falls back to `shell_pid` when tpgid is unavailable or zero.
pub(super) fn cwd_tracking_pid(shell_pid: i32) -> i32 {
    if let Some((_pgrp, tpgid)) = parse_pgrp_tpgid(shell_pid) {
        if tpgid > 0 {
            return tpgid;
        }
    }
    shell_pid
}

/// Compact relative-duration format: `45s`, `3m`, `1h 12m`.
pub(super) fn format_duration_compact(d: Duration) -> String {
    let s = d.as_secs();
    if s < 60 {
        format!("{}s", s)
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else {
        format!("{}h {}m", s / 3600, (s / 60) % 60)
    }
}
