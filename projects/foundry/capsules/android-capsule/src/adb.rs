use crate::{Error, Result};
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::process::Command;

async fn run(args: &[&str]) -> Result<std::process::Output> {
    Command::new("adb")
        .args(args)
        .output()
        .await
        .map_err(|error| Error::Unavailable(format!("adb: {error}")))
}

pub async fn connect(serial: &str) -> Result<()> {
    let output = run(&["connect", serial]).await?;
    if !output.status.success() {
        return Err(Error::Backend(format!(
            "adb connect: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

pub async fn wait_for_boot(serial: &str, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let output = run(&["-s", serial, "shell", "getprop", "sys.boot_completed"]).await;
        if let Ok(output) = output {
            if output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "1" {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err(Error::NotReady(format!(
                "Android did not finish booting within {timeout:?}"
            )));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

pub async fn screenshot(serial: &str) -> Result<Vec<u8>> {
    let output = run(&["-s", serial, "exec-out", "screencap", "-p"]).await?;
    if !output.status.success() {
        return Err(Error::Backend(format!(
            "screencap: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    if !output.stdout.starts_with(b"\x89PNG") {
        return Err(Error::Backend("screencap returned non-PNG data".into()));
    }
    Ok(output.stdout)
}

pub async fn tap(serial: &str, x: i32, y: i32) -> Result<()> {
    shell(serial, &format!("input tap {x} {y}")).await
}

pub async fn swipe(
    serial: &str,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    duration_ms: u32,
) -> Result<()> {
    shell(
        serial,
        &format!("input swipe {x1} {y1} {x2} {y2} {duration_ms}"),
    )
    .await
}

pub async fn text(serial: &str, text: &str) -> Result<()> {
    let quoted = quote_text(text)?;
    shell(serial, &format!("input text {quoted}")).await
}

pub async fn key(serial: &str, key: &str) -> Result<()> {
    let code = keycode(key).ok_or_else(|| Error::InvalidRequest(format!("unknown key {key}")))?;
    shell(serial, &format!("input keyevent {code}")).await
}

pub async fn install_apk(serial: &str, local: &Path) -> Result<()> {
    let path = local
        .to_str()
        .ok_or_else(|| Error::InvalidRequest("APK path is not UTF-8".into()))?;
    let output = run(&["-s", serial, "install", "-r", path]).await?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() || !text.contains("Success") {
        return Err(Error::Backend(format!("adb install: {}", text.trim())));
    }
    Ok(())
}

pub async fn launch(serial: &str, package: &str) -> Result<()> {
    if package.is_empty()
        || !package.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '.' || character == '_'
        })
    {
        return Err(Error::InvalidRequest(format!(
            "invalid Android package name: {package}"
        )));
    }
    shell(
        serial,
        &format!("monkey -p {package} -c android.intent.category.LAUNCHER 1"),
    )
    .await
}

pub async fn emu_kill(serial: &str) -> Result<()> {
    let _ = run(&["-s", serial, "emu", "kill"]).await?;
    Ok(())
}

pub async fn disconnect(serial: &str) -> Result<()> {
    let _ = run(&["disconnect", serial]).await?;
    Ok(())
}

async fn shell(serial: &str, command: &str) -> Result<()> {
    let output = run(&["-s", serial, "shell", command]).await?;
    if !output.status.success() {
        return Err(Error::Backend(format!(
            "adb shell {command}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

fn quote_text(text: &str) -> Result<String> {
    if text.contains('\n') || text.contains('\r') {
        return Err(Error::InvalidRequest(
            "Android input text only supports a single line".into(),
        ));
    }
    let encoded = text.replace(' ', "%s");
    Ok(format!("'{}'", encoded.replace('\'', "'\\''")))
}

fn keycode(key: &str) -> Option<i32> {
    let code = match key {
        "Home" => 3,
        "Back" => 4,
        "Call" => 5,
        "EndCall" => 6,
        "VolumeUp" => 24,
        "VolumeDown" => 25,
        "Power" => 26,
        "Camera" => 27,
        "Clear" => 28,
        "Tab" => 61,
        "Space" => 62,
        "Return" | "Enter" => 66,
        "Delete" | "Del" => 67,
        "Menu" => 82,
        "Search" => 84,
        "PageUp" => 92,
        "PageDown" => 93,
        "Escape" | "Esc" => 111,
        "Forward" => 125,
        "Up" | "DPadUp" | "ArrowUp" => 19,
        "Down" | "DPadDown" | "ArrowDown" => 20,
        "Left" | "DPadLeft" | "ArrowLeft" => 21,
        "Right" | "DPadRight" | "ArrowRight" => 22,
        "Center" | "DPadCenter" => 23,
        _ => return None,
    };
    Some(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_text_for_adb_shell() {
        assert_eq!(quote_text("hello world").unwrap(), "'hello%sworld'");
        assert_eq!(quote_text("it's ok").unwrap(), "'it'\\''s%sok'");
        assert!(quote_text("a\nb").is_err());
    }

    #[test]
    fn maps_android_keys() {
        assert_eq!(keycode("Back"), Some(4));
        assert_eq!(keycode("Return"), Some(66));
        assert_eq!(keycode("ArrowUp"), Some(19));
        assert_eq!(keycode("NotAKey"), None);
    }
}
