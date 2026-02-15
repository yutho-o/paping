use std::path::{Path, PathBuf};

pub enum InstallOutcome {
    /// Already running from the intended install location.
    Noop,
    /// Installed and relaunched from the install location.
    Relaunched,
}

pub fn ensure_installed_and_relaunch_if_needed() -> Result<InstallOutcome, String> {
    let current_exe = std::env::current_exe().map_err(|e| format!("current_exe failed: {e}"))?;
    let install_exe = desired_install_exe_path()?;

    // If we're already running from the install location, do nothing.
    if same_path(&current_exe, &install_exe) {
        return Ok(InstallOutcome::Noop);
    }

    // Prevent loops in case something weird happens.
    if std::env::var_os("PAPING_NO_AUTO_INSTALL").is_some() {
        return Ok(InstallOutcome::Noop);
    }

    if let Some(parent) = install_exe.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create install dir '{}': {e}", parent.display()))?;
    }

    // Copy current binary to install location (overwrite).
    std::fs::copy(&current_exe, &install_exe).map_err(|e| {
        format!(
            "Failed to copy '{}' to '{}': {e}",
            current_exe.display(),
            install_exe.display()
        )
    })?;

    // Make executable on Unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&install_exe)
            .map_err(|e| format!("metadata failed for '{}': {e}", install_exe.display()))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&install_exe, perms).map_err(|e| {
            format!(
                "Failed to set permissions on '{}': {e}",
                install_exe.display()
            )
        })?;
    }

    // Best-effort: add install dir to PATH.
    if let Some(dir) = install_exe.parent() {
        add_to_user_path(dir);
    }

    // Relaunch from installed location with the same args.
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let mut cmd = std::process::Command::new(&install_exe);
    cmd.args(args);
    // Disable auto-install in the child process to avoid any accidental loops.
    cmd.env("PAPING_NO_AUTO_INSTALL", "1");

    cmd.spawn().map_err(|e| {
        format!(
            "Failed to relaunch from '{}': {e}",
            install_exe.display()
        )
    })?;

    Ok(InstallOutcome::Relaunched)
}

fn desired_install_exe_path() -> Result<PathBuf, String> {
    let exe_name = if cfg!(windows) { "paping.exe" } else { "paping" };
    let dir = desired_install_dir()?;
    Ok(dir.join(exe_name))
}

fn desired_install_dir() -> Result<PathBuf, String> {
    if cfg!(windows) {
        // Prefer LocalAppData, fallback to AppData.
        let base = std::env::var_os("LOCALAPPDATA")
            .or_else(|| std::env::var_os("APPDATA"))
            .ok_or("LOCALAPPDATA/APPDATA not set".to_string())?;
        Ok(PathBuf::from(base).join("paping").join("bin"))
    } else {
        // ~/.local/bin
        let home = std::env::var_os("HOME").ok_or("HOME not set".to_string())?;
        Ok(PathBuf::from(home).join(".local").join("bin"))
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    // Canonicalize if possible, otherwise fallback to raw compare.
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn add_to_user_path(dir: &Path) {
    #[cfg(windows)]
    {
        if let Err(e) = add_to_user_path_windows(dir) {
            eprintln!("Warning: failed to add to PATH automatically: {e}");
            eprintln!(
                "You can add this directory to PATH manually: {}",
                dir.display()
            );
        }
    }

    #[cfg(not(windows))]
    {
        // Modifying shell startup files automatically is risky and shell-dependent.
        // We'll just print a hint if it doesn't look like it's already in PATH.
        let dir_str = dir.to_string_lossy();
        let path = std::env::var("PATH").unwrap_or_default();
        if !path.split(':').any(|p| p == dir_str) {
            eprintln!("Note: '{}' is not in PATH.", dir.display());
            eprintln!("Add it to your shell profile to call 'paping' anywhere.");
        }
    }
}

#[cfg(windows)]
fn add_to_user_path_windows(dir: &Path) -> Result<(), String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    let dir_str = dir
        .to_str()
        .ok_or_else(|| "Install directory is not valid UTF-8".to_string())?;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (env_key, _) = hkcu
        .create_subkey("Environment")
        .map_err(|e| format!("registry open failed: {e}"))?;

    let current: String = env_key.get_value("Path").unwrap_or_default();

    // Avoid duplicates (case-insensitive).
    let already = current
        .split(';')
        .any(|p| p.eq_ignore_ascii_case(dir_str));

    if already {
        return Ok(());
    }

    let new_path = if current.trim().is_empty() {
        dir_str.to_string()
    } else {
        format!("{};{}", current.trim_end_matches(';'), dir_str)
    };

    // Must open with write rights.
    let env_key = hkcu
        .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)
        .map_err(|e| format!("registry open (write) failed: {e}"))?;

    env_key
        .set_value("Path", &new_path)
        .map_err(|e| format!("registry write failed: {e}"))?;

    // We intentionally don't broadcast WM_SETTINGCHANGE here.
    // A new terminal/session will pick up the updated PATH.
    Ok(())
}
