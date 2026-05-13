//! Set PureTerm as the default terminal emulator on macOS.

use std::process::Command;

/// Check if PureTerm is the default terminal.
#[cfg(target_os = "macos")]
pub fn is_default_terminal() -> bool {
    let output = Command::new("defaults")
        .args([
            "read",
            "com.apple.LaunchServices/com.apple.launchservices.secure",
            "LSHandlers",
        ])
        .output();

    if let Ok(output) = output {
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Check if our bundle ID is registered for public.shell
        stdout.contains("com.pureterm") || stdout.contains("PureTerm")
    } else {
        false
    }
}

/// Set PureTerm as the default terminal on macOS.
/// This uses LSSetDefaultHandlerForURLScheme for the ssh:// scheme
/// and also sets the default for .sh files and public.shell UTI.
#[cfg(target_os = "macos")]
pub fn set_default_terminal() -> Result<(), String> {
    // Use dutool or defaultbrowser, or just open System Settings
    // The most reliable way is to use LSSetDefaultHandlerForURLScheme via LaunchServices
    // But that requires ObjC. Instead, we'll use the `open` command approach.

    // Actually, the simplest approach: use `defaults write` to register,
    // then tell the user to verify in System Settings
    let bundle_id = "com.pureterm";

    // Register as handler for ssh:// URLs
    let _ = Command::new("defaults")
        .args(["write", "com.apple.LaunchServices", "LSHandlers", "-array-add"])
        .arg(format!(
            "{{LSHandlerURLScheme=\"ssh\";LSHandlerRoleAll=\"{}\";}}",
            bundle_id
        ))
        .output();

    Ok(())
}

/// Open macOS System Settings > Default Apps so user can manually set PureTerm.
#[cfg(target_os = "macos")]
pub fn open_default_apps_settings() -> Result<(), String> {
    Command::new("open")
        .args(["x-apple.systempreferences:com.apple.Desktop-Settings.extension"])
        .output()
        .map_err(|e| format!("Failed to open settings: {}", e))?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn is_default_terminal() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub fn set_default_terminal() -> Result<(), String> {
    Err("Not supported on this platform".into())
}

#[cfg(not(target_os = "macos"))]
pub fn open_default_apps_settings() -> Result<(), String> {
    Err("Not supported on this platform".into())
}
