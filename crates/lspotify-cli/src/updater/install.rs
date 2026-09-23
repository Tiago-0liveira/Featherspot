use std::path::Path;

use super::UpdateError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReplacementResult {
    DirectlyReplaced,
    HelperSpawned(String),
}

/// Safely replaces an existing executable with a new binary.
///
/// On Windows, if replacing the current running process, spawns a helper and returns `HelperSpawned`.
/// Otherwise, performs safe direct replacement with backup and rollback.
///
/// # Errors
/// Returns `UpdateError::Install` if the binary does not exist or replacement fails.
pub fn safe_replace_executable(
    target_path: &Path,
    new_binary_path: &Path,
) -> Result<ReplacementResult, UpdateError> {
    if !new_binary_path.exists() {
        return Err(UpdateError::Install(format!(
            "Replacement binary does not exist at {}",
            new_binary_path.display()
        )));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        let _ = std::fs::set_permissions(new_binary_path, perms);
    }

    #[cfg(target_os = "windows")]
    {
        let is_running_binary = std::env::current_exe().ok().and_then(|current| {
            let canon_current = std::fs::canonicalize(&current).ok()?;
            let canon_target = std::fs::canonicalize(target_path).ok()?;
            Some(canon_current == canon_target)
        });

        if is_running_binary.unwrap_or(false) {
            let helper_info = spawn_windows_replacement_helper(target_path, new_binary_path)?;
            return Ok(ReplacementResult::HelperSpawned(helper_info));
        }
    }

    direct_safe_replace(target_path, new_binary_path)?;
    Ok(ReplacementResult::DirectlyReplaced)
}

/// Directly replaces `target` with `replacement`, retaining a backup and rolling back on error.
///
/// # Errors
/// Returns `UpdateError::Install` if file operations or replacement fails.
pub fn direct_safe_replace(target: &Path, replacement: &Path) -> Result<(), UpdateError> {
    let backup = target.with_extension("bak");
    let had_target = target.exists();

    if had_target {
        if backup.exists() {
            let _ = std::fs::remove_file(&backup);
        }
        std::fs::rename(target, &backup).map_err(|err| {
            UpdateError::Install(format!(
                "Failed to rename target '{}' to backup '{}': {err}",
                target.display(),
                backup.display()
            ))
        })?;
    }

    let copy_result = std::fs::copy(replacement, target).map(|_| ());
    #[cfg(unix)]
    let copy_result = copy_result.and_then(|()| {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o755))
    });

    if let Err(err) = copy_result {
        // Rollback
        if had_target && backup.exists() {
            let _ = std::fs::rename(&backup, target);
        }
        return Err(UpdateError::Install(format!("Failed to install replacement binary: {err}")));
    }

    // Success: remove backup and replacement
    if had_target && backup.exists() {
        let _ = std::fs::remove_file(&backup);
    }
    let _ = std::fs::remove_file(replacement);
    Ok(())
}

#[cfg(target_os = "windows")]
/// Spawns a background helper script to replace the executable after process termination.
///
/// # Errors
/// Returns `UpdateError::Install` if the script cannot be written or helper cannot be spawned.
pub fn spawn_windows_replacement_helper(
    target_exe: &Path,
    new_exe: &Path,
) -> Result<String, UpdateError> {
    let pid = std::process::id();
    let temp_dir = new_exe
        .parent()
        .or_else(|| target_exe.parent())
        .ok_or_else(|| UpdateError::Install("Could not determine temporary directory".into()))?;

    let script_path = temp_dir.join("lspotify_update.ps1");

    let script_content = format!(
        r#"$ErrorActionPreference = 'SilentlyContinue'
$target = "{target}"
$replacement = "{replacement}"
$pidToWait = {pid}
$backup = "$target.bak"

if ($pidToWait -gt 0) {{
    Wait-Process -Id $pidToWait -Timeout 30 -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 500
}}

if (Test-Path $backup) {{ Remove-Item -Force $backup }}
Move-Item -Force $target $backup
Copy-Item -Force $replacement $target

if (Test-Path $target) {{
    Remove-Item -Force $backup
    Remove-Item -Force $replacement
}} else {{
    Move-Item -Force $backup $target
}}
"#,
        target = target_exe.display(),
        replacement = new_exe.display(),
        pid = pid,
    );

    std::fs::write(&script_path, script_content)
        .map_err(|err| UpdateError::Install(format!("Failed to write updater script: {err}")))?;

    let mut command = std::process::Command::new("powershell.exe");
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-WindowStyle",
        "Hidden",
        "-File",
        &script_path.to_string_lossy(),
    ]);

    command
        .spawn()
        .map_err(|err| UpdateError::Install(format!("Failed to spawn updater helper: {err}")))?;

    Ok(format!("Updater helper spawned for PID {pid}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_safe_replace_replaces_file_cleanly() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("lspotify.exe");
        let replacement = temp.path().join("new_lspotify.exe");

        std::fs::write(&target, b"version 0.4.2").unwrap();
        std::fs::write(&replacement, b"version 0.5.0").unwrap();

        let res = safe_replace_executable(&target, &replacement).unwrap();
        assert_eq!(res, ReplacementResult::DirectlyReplaced);

        assert_eq!(std::fs::read(&target).unwrap(), b"version 0.5.0");
        assert!(!replacement.exists());
        assert!(!target.with_extension("bak").exists());
    }

    #[test]
    fn direct_safe_replace_rolls_back_on_error() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("lspotify.exe");
        let non_existent = temp.path().join("missing.exe");

        std::fs::write(&target, b"version 0.4.2").unwrap();

        let res = safe_replace_executable(&target, &non_existent);
        assert!(res.is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"version 0.4.2");
    }
}
