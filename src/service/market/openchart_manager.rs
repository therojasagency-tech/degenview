//! On-demand local bridge supervisor for the desktop application. No provider credentials on disk.
use crate::service::{ProviderError, keychain::openchart as credentials};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Mutex, OnceLock, mpsc},
    time::{Duration, Instant},
};

#[derive(Clone, Serialize, Deserialize)]
pub struct Settings {
    pub checkout: PathBuf,
    #[serde(default)]
    pub authenticated: bool,
}
fn settings_path() -> Option<PathBuf> {
    Some(crate::paths::data_dir()?.join("openchart-local-v1.json"))
}
pub fn saved_settings() -> Option<Settings> {
    serde_json::from_slice(&std::fs::read(settings_path()?).ok()?).ok()
}
pub fn default_checkout() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join("Desktop/openchart")
}
pub fn save_settings(checkout: PathBuf, authenticated: bool) -> Result<(), String> {
    if !checkout.is_absolute() || !checkout.join("node_modules").is_dir() || !checkout.join("server/data/providers/tradingview/client.ts").is_file() {
        return Err("Selecciona el checkout de OpenChart con sus dependencias instaladas.".into());
    }
    let path = settings_path().ok_or("No se encontró el directorio de datos.")?;
    std::fs::create_dir_all(path.parent().ok_or("Invalid settings directory")?).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(&Settings { checkout, authenticated }).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}
pub fn remove_settings() -> Result<(), String> {
    let path = settings_path().ok_or("No se encontró el directorio de datos.")?;
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[derive(Clone, Deserialize)]
struct Ready {
    version: u32,
    port: u16,
    token: String,
}
fn parse_ready(line: &str) -> Option<Ready> {
    let ready: Ready = serde_json::from_str(line).ok()?;
    (ready.version == 1 && ready.port != 0 && ready.token.len() == 64
        && ready.token.bytes().all(|b| b.is_ascii_hexdigit())).then_some(ready)
}
struct Running {
    child: Child,
    ready: Ready,
}
impl Drop for Running {
    fn drop(&mut self) {
        // EOF tells the Bun child to stop, then the shell removes its staging directory.
        self.child.stdin.take();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) { return; }
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[derive(Default)]
struct State {
    running: Option<Running>,
    failed: Option<Instant>,
}
static STATE: Mutex<State> = Mutex::new(State { running: None, failed: None });
static SETTINGS: OnceLock<Option<Settings>> = OnceLock::new();

fn helper(settings: &Settings, mode: &str) -> Result<Child, ProviderError> {
    let error = || ProviderError::other("OpenChart could not start. Check its checkout and Bun installation in Settings.");
    let exe = std::env::current_exe().map_err(|_| error())?;
    let script = exe.parent().and_then(|p| p.parent()).ok_or_else(error)?
        .join("Resources/data-bridge/run-openchart.sh");
    let bun = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(error)?.join(".bun/bin");
    let path = format!("{}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin", bun.display());
    Command::new("/bin/bash")
        .arg(script).arg(&settings.checkout).arg("--managed")
        .env_clear().env("PATH", path)
        .env("HOME", std::env::var_os("HOME").ok_or_else(error)?)
        .env("DEGENVIEW_TV_AUTHORIZED", "1")
        .env("DEGENVIEW_TV_LOGIN", mode)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
        .spawn().map_err(|_| error())
}

fn start(settings: &Settings) -> Result<Running, ProviderError> {
    let error = || ProviderError::other("OpenChart could not start. Check its checkout, Bun and saved TradingView session in Settings.");
    let session = if settings.authenticated {
        Some(credentials::read().ok_or_else(|| ProviderError::other("TradingView saved session unavailable. Reconnect in Settings."))?)
    } else { None };
    let mut child = helper(settings, if session.is_some() { "saved" } else { "anonymous" })?;
    if let Some(session) = session {
        let written = child.stdin.as_mut().is_some_and(|stdin| writeln!(stdin, "{session}").is_ok() && stdin.flush().is_ok());
        if !written { child.stdin.take(); let _ = child.kill(); let _ = child.wait(); return Err(error()); }
    }
    let stdout = child.stdout.take().ok_or_else(error)?;
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut lines = BufReader::new(stdout).lines();
        let ready = lines.next().and_then(Result::ok).and_then(|line| parse_ready(&line));
        let _ = sender.send(ready);
        // Drain any later diagnostics without logging them.
        for _ in lines {}
    });
    let ready = receiver.recv_timeout(Duration::from_secs(20)).ok().flatten();
    match ready {
        Some(ready) => Ok(Running { child, ready }),
        None => {
            child.stdin.take();
            let _ = child.kill();
            let _ = child.wait();
            Err(error())
        }
    }
}


/// Browser login is a separate short-lived helper so a failed or cancelled attempt cannot
/// replace the session currently in use. Its only stdout is the private session envelope.
pub fn connect_account(checkout: PathBuf) -> Result<(), String> {
    if !checkout.is_absolute() || !checkout.join("node_modules").is_dir() || !checkout.join("server/data/providers/tradingview/client.ts").is_file() {
        return Err("Selecciona el checkout de OpenChart con sus dependencias instaladas.".into());
    }
    let settings = Settings { checkout, authenticated: true };
    let mut child = helper(&settings, "enroll").map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().ok_or("No se pudo abrir el canal privado.")?;
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let result = reader.read_line(&mut line).ok().filter(|n| *n > 0 && *n < 32768).map(|_| line);
        let _ = sender.send(result);
    });
    let line = receiver.recv_timeout(Duration::from_secs(370)).ok().flatten();
    child.stdin.take();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if matches!(child.try_wait(), Ok(Some(_))) { break; }
        if Instant::now() >= deadline { let _ = child.kill(); let _ = child.wait(); break; }
        std::thread::sleep(Duration::from_millis(25));
    }
    #[derive(Deserialize)]
    struct Enrollment { session: serde_json::Value }
    let enrollment: Enrollment = serde_json::from_str(&line.ok_or("Inicio de sesión cancelado o no disponible.")?)
        .map_err(|_| "No se pudo validar la sesión de TradingView.")?;
    if !enrollment.session.get("sessionid").and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty()) {
        return Err("TradingView no devolvió una sesión válida.".into());
    }
    let session = serde_json::to_string(&enrollment.session).map_err(|_| "No se pudo preparar la sesión.")?;
    credentials::save(&session)?;
    save_settings(settings.checkout, true)
}

pub fn disconnect_account() -> Result<(), String> {
    credentials::remove()?;
    // Do not re-enable a disabled provider or adopt an unconfirmed new checkout.
    if let Some(settings) = saved_settings() {
        save_settings(settings.checkout, false)?;
    }
    Ok(())
}

/// External launchers retain priority. The internal helper starts on the first market request,
/// never on the UI thread. Preferences are intentionally applied only after an app restart.
pub fn endpoint() -> Result<(u16, String), ProviderError> {
    let port = std::env::var("DEGENVIEW_BRIDGE_PORT").ok().and_then(|v| v.parse::<u16>().ok()).filter(|p| *p != 0);
    let token = std::env::var("DEGENVIEW_BRIDGE_TOKEN").ok().filter(|t| t.len() >= 32 && t.bytes().all(|b| b.is_ascii_graphic()));
    if let (Some(port), Some(token)) = (port, token) { return Ok((port, token)); }
    if cfg!(test) || !cfg!(target_os = "macos") {
        return Err(ProviderError::other("Start the local OpenChart bridge."));
    }
    let settings = SETTINGS.get_or_init(saved_settings).as_ref()
        .ok_or_else(|| ProviderError::other("Enable TradingView in Settings and restart DegenView."))?;
    let mut state = crate::lock::lock(&STATE);
    if let Some(running) = state.running.as_mut() {
        if matches!(running.child.try_wait(), Ok(None)) {
            return Ok((running.ready.port, running.ready.token.clone()));
        }
    }
    state.running.take();
    if state.failed.is_some_and(|at| at.elapsed() < Duration::from_secs(15)) {
        return Err(ProviderError::other("OpenChart is waiting to reconnect."));
    }
    match start(settings) {
        Ok(running) => {
            let endpoint = (running.ready.port, running.ready.token.clone());
            state.running = Some(running);
            state.failed = None;
            Ok(endpoint)
        }
        Err(error) => { state.failed = Some(Instant::now()); Err(error) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_valid_local_handshake_is_accepted() {
        assert!(parse_ready(&format!(r#"{{"version":1,"port":1234,"token":"{}"}}"#, "a".repeat(64))).is_some());
        assert!(parse_ready(r#"{"version":1,"port":0,"token":"bad"}"#).is_none());
        assert!(parse_ready("unexpected diagnostic").is_none());
    }
}
