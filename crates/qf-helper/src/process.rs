use std::path::Path;

pub const WARFRAME_EXE: &str = "Warframe.x64.exe";

/// True when one NUL-separated argument's file name (after the last `/` or `\`) is `Warframe.x64.exe`,
/// ignoring ASCII case. Shell commands that merely mention the name don't match (amendment D7).
pub fn is_warframe_cmdline(cmdline: &[u8]) -> bool {
    cmdline.split(|b| *b == 0).filter(|arg| !arg.is_empty()).any(|arg| {
        let arg = String::from_utf8_lossy(arg);
        arg.rsplit(|c| c == '/' || c == '\\').next().is_some_and(|name| name.eq_ignore_ascii_case(WARFRAME_EXE))
    })
}

/// Scans `<proc_root>/<pid>/cmdline` for Warframe, skipping `own_pid`.
pub fn warframe_running(proc_root: &Path, own_pid: u32) -> bool {
    let Ok(entries) = std::fs::read_dir(proc_root) else { return false };
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_str().and_then(|name| name.parse::<u32>().ok()).is_some_and(|pid| pid != own_pid))
        .any(|entry| std::fs::read(entry.path().join("cmdline")).is_ok_and(|cmdline| is_warframe_cmdline(&cmdline)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<u8> {
        parts.join("\0").into_bytes()
    }

    #[test]
    fn matches_the_game_executable_under_proton_or_unix_paths() {
        assert!(is_warframe_cmdline(&args(&[r"Z:\home\player\.local\share\Steam\steamapps\common\Warframe\Downloaded\Public\Warframe.x64.exe", "-cluster:public"])));
        assert!(is_warframe_cmdline(&args(&["/usr/bin/wine64-preloader", "C:/Program Files/Warframe/warframe.x64.exe"])));
        assert!(is_warframe_cmdline(&args(&["Warframe.x64.exe"])));
    }

    #[test]
    fn mentions_and_similar_names_do_not_match() {
        assert!(!is_warframe_cmdline(&args(&["/usr/bin/bash", "-c", "pgrep -af 'Warframe.x64.exe' | head"])));
        assert!(!is_warframe_cmdline(&args(&["tail", "/tmp/Warframe.x64.exe.log"])));
        assert!(!is_warframe_cmdline(&args(&[r"Z:\Warframe\Tools\Launcher.exe"])));
        assert!(!is_warframe_cmdline(b""));
    }

    #[test]
    fn scans_proc_and_skips_its_own_process() {
        let root = tempfile::tempdir().unwrap();
        for (pid, cmdline) in [
            ("100", args(&["/usr/bin/bash", "-c", "pgrep Warframe.x64.exe"])),
            ("200", args(&[r"Z:\Warframe\Warframe.x64.exe"])),
            ("self", args(&[r"Z:\Warframe\Warframe.x64.exe"])),
        ] {
            std::fs::create_dir_all(root.path().join(pid)).unwrap();
            std::fs::write(root.path().join(pid).join("cmdline"), cmdline).unwrap();
        }
        assert!(warframe_running(root.path(), 1));
        assert!(!warframe_running(root.path(), 200), "own pid is skipped; `self` is not a pid");
        assert!(!warframe_running(&root.path().join("missing"), 1));
    }
}
