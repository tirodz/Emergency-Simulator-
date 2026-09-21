use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use tauri::{Emitter, Manager, State};

const SERVICE_CATEGORY: u32 = 4355;
const REQUIRED_PREFIX: &str = "TEST";
const DEFAULT_BODY: &str = "TEST ALERT - SIMULATION";
const INJECTOR_CLASS: &str = "org.emergencysim.alertinject.AlertInjector";
const INJECTOR_REMOTE: &str = "/data/local/tmp/alertinject.jar";
const EVIDENCE_TIMEOUT_SECS: u64 = 45;
const LOCAL_SIMULATOR_PACKAGE: &str = "com.tirodz.emergencysimulator";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeviceState {
    Ready,
    SimulatorReady,
    Unauthorized,
    Offline,
    NoRoot,
    Unsupported,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SupportLevel {
    Supported,
    LocalSimulator,
    RootRequired,
    Untested,
    Unsupported,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceSpecs {
    pub cpu: Option<String>,
    pub ram_gb: Option<f32>,
    pub storage_gb: Option<f32>,
    pub battery_percent: Option<u8>,
    pub screen_resolution: Option<String>,
    pub density: Option<u32>,
    pub announced: Option<String>,
    pub dimensions: Option<String>,
    pub weight_g: Option<u32>,
    pub memory_options: Option<String>,
    pub storage_options: Option<String>,
    pub display_profile: Option<String>,
    pub battery_capacity_mah: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Device {
    pub serial: String,
    pub model: Option<String>,
    pub product: Option<String>,
    pub manufacturer: Option<String>,
    pub release: Option<String>,
    pub sdk: Option<String>,
    pub build_type: Option<String>,
    pub debuggable: Option<String>,
    pub root: bool,
    pub cellbroadcast_package: Option<String>,
    pub cellbroadcast_candidates: Vec<String>,
    pub local_simulator: bool,
    pub state: DeviceState,
    pub support_level: SupportLevel,
    pub specs: DeviceSpecs,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppInfo {
    pub version: String,
    pub adb_source: String,
    pub injector: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SendResult {
    pub device_serial: String,
    pub category: u32,
    pub body: String,
    pub state: String,
    pub failure: Option<String>,
    pub message: String,
    pub evidence: Vec<String>,
    pub injector_exit_code: Option<i32>,
    pub diagnostics: Vec<DiagEvent>,
    /// The pipeline stage that actually failed, so the UI can name it instead of showing a
    /// generic error. `None` when the run did not fail.
    pub failed_stage: Option<String>,
}

/// One structured diagnostic event.
///
/// Every event carries enough context to debug from the Activity panel alone: which stage, which
/// device, what command was issued, and the raw stdout/stderr. Nothing is collapsed into a generic
/// message, because the whole point of the project is that an absence of evidence must be
/// distinguishable from evidence of success.
#[derive(Debug, Clone, Serialize)]
pub struct DiagEvent {
    pub stage: String,
    pub serial: String,
    pub action: String,
    pub detail: Option<String>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize)]
struct ActivityEvent {
    message: String,
    kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum TxState {
    Busy,
    Delivered,
    Uncertain,
}

#[derive(Debug, Default)]
struct TxStore {
    map: Mutex<HashMap<String, TxState>>,
    error: Mutex<Option<String>>,
}

#[derive(Debug, Default)]
struct CancelStore {
    set: Mutex<HashSet<String>>,
}

type SharedTx = Arc<TxStore>;
type SharedCancel = Arc<CancelStore>;

fn emit_log(app: &tauri::AppHandle, message: impl Into<String>, kind: &str) {
    let _ = app.emit(
        "activity",
        ActivityEvent {
            message: message.into(),
            kind: kind.to_string(),
        },
    );
}

/// Stage names, kept in one place so the Android side and the Rust side cannot drift apart.
///
/// `AlertStages.kt` emits these as `EMSIM:STAGE=<NAME>`; the constants below are the Rust half of
/// that contract. A change to one must change the other.
mod stage {
    pub const TEST_CREATED: &str = "TEST_CREATED";
    pub const DEVICE_SELECTED: &str = "DEVICE_SELECTED";
    pub const DEVICE_CHECK: &str = "DEVICE_CHECK";
    pub const LOCAL_SIMULATOR_CHECK: &str = "LOCAL_SIMULATOR_CHECK";
    pub const LOCAL_SIMULATOR_INSTALL: &str = "LOCAL_SIMULATOR_INSTALL";
    pub const CAPABILITY_CHECK: &str = "CAPABILITY_CHECK";
    pub const ADB_BROADCAST_DISPATCH: &str = "ADB_BROADCAST_DISPATCH";
    pub const ADB_BROADCAST_RESULT: &str = "ADB_BROADCAST_RESULT";
    pub const TEST_COMPLETE: &str = "TEST_COMPLETE";
    pub const TEST_FAILED: &str = "TEST_FAILED";
}

/// Prefix the Android companion writes for every pipeline stage.
const ANDROID_STAGE_PREFIX: &str = "EMSIM:STAGE=";

fn now_timestamp() -> String {
    // Milliseconds since the Unix epoch. Rendered as UTC by the frontend, so no timezone
    // dependency and no locale-sensitive formatting on the Rust side.
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => duration.as_millis().to_string(),
        Err(_) => "0".to_string(),
    }
}

/// Record one diagnostic event and mirror it into the Activity panel.
///
/// The Activity line is the operator-facing summary; the `DiagEvent` is the structured record the
/// UI can inspect in full, including the stderr that a summary would drop.
#[allow(clippy::too_many_arguments)]
fn diag(
    app: &tauri::AppHandle,
    result: &mut SendResult,
    stage: &str,
    action: impl Into<String>,
    detail: Option<String>,
    stdout: Option<String>,
    stderr: Option<String>,
    kind: &str,
) {
    let action = action.into();
    let summary = match &detail {
        Some(detail) => format!("{stage} · {action} · {detail}"),
        None => format!("{stage} · {action}"),
    };
    emit_log(app, summary, kind);

    result.diagnostics.push(DiagEvent {
        stage: stage.to_string(),
        serial: result.device_serial.clone(),
        action,
        detail,
        stdout,
        stderr,
        timestamp: now_timestamp(),
    });
}

/// Record a failure at a named stage and mark the result.
fn fail_stage(
    app: &tauri::AppHandle,
    result: &mut SendResult,
    stage: &str,
    failure_code: &str,
    action: impl Into<String>,
    message: impl Into<String>,
    stdout: Option<String>,
    stderr: Option<String>,
) {
    let message = message.into();
    result.failure = Some(failure_code.to_string());
    result.failed_stage = Some(stage.to_string());
    result.message = message.clone();
    diag(
        app,
        result,
        stage,
        action,
        Some(message),
        stdout,
        stderr,
        "error",
    );
}

/// Extract every `EMSIM:STAGE=<NAME>` token present in a logcat dump, in order of first
/// appearance, with any trailing `key=value` detail preserved.
///
/// Matching a stable token rather than prose is what makes the evidence durable: the Android
/// companion can reword its human-readable logging without silently breaking delivery detection.
fn parse_android_stages(dump: &str) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();

    for line in dump.lines() {
        let Some(index) = line.find(ANDROID_STAGE_PREFIX) else {
            continue;
        };
        let tail = &line[index + ANDROID_STAGE_PREFIX.len()..];
        let mut parts = tail.splitn(2, ' ');
        let name = parts.next().unwrap_or("").trim().to_string();
        if name.is_empty() {
            continue;
        }
        let detail = parts.next().unwrap_or("").trim().to_string();
        if !found.iter().any(|(existing, _)| *existing == name) {
            found.push((name, detail));
        }
    }

    found
}

fn data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("Could not resolve application data directory: {e}"))
}

fn tx_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("transactions.json"))
}

fn load_transactions(app: &tauri::AppHandle, store: &TxStore) {
    let path = match tx_path(app) {
        Ok(path) => path,
        Err(error) => {
            *store.error.lock().unwrap() = Some(error);
            return;
        }
    };

    if !path.exists() {
        return;
    }

    let parsed = fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<HashMap<String, TxState>>(&text).ok());

    match parsed {
        Some(mut map) => {
            for value in map.values_mut() {
                if matches!(value, TxState::Busy) {
                    *value = TxState::Uncertain;
                }
            }
            *store.map.lock().unwrap() = map;
        }
        None => {
            *store.error.lock().unwrap() = Some(format!(
                "The local safety state at {} could not be parsed. Sending is locked until it is reset after manually checking the device.",
                path.display()
            ));
        }
    }
}

fn persist_transactions(app: &tauri::AppHandle, store: &TxStore) -> Result<(), String> {
    if let Some(error) = store.error.lock().unwrap().clone() {
        return Err(error);
    }

    let path = tx_path(app)?;
    let parent = path
        .parent()
        .ok_or_else(|| "Transaction path has no parent".to_string())?;

    fs::create_dir_all(parent).map_err(|e| e.to_string())?;

    let encoded = {
        let map = store.map.lock().unwrap();
        serde_json::to_vec_pretty(&*map).map_err(|e| e.to_string())?
    };

    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, encoded).map_err(|e| e.to_string())?;

    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
    }

    fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(())
}

fn set_tx(
    app: &tauri::AppHandle,
    store: &TxStore,
    serial: &str,
    state: Option<TxState>,
) -> Result<(), String> {
    if let Some(error) = store.error.lock().unwrap().clone() {
        return Err(error);
    }

    {
        let mut map = store.map.lock().unwrap();
        match state {
            Some(value) => {
                map.insert(serial.to_string(), value);
            }
            None => {
                map.remove(serial);
            }
        }
    }

    persist_transactions(app, store)
}

fn gate_for(store: &TxStore, serial: &str) -> Option<String> {
    if let Some(error) = store.error.lock().unwrap().clone() {
        return Some(error);
    }

    match store.map.lock().unwrap().get(serial) {
        None => None,
        Some(TxState::Busy) => Some(
            "A test alert is already being processed for this device."
                .to_string(),
        ),
        Some(TxState::Delivered) => Some(
            "The previous Android test alert is still outstanding. Dismiss it on the phone, then acknowledge it here before sending again."
                .to_string(),
        ),
        Some(TxState::Uncertain) => Some(
            "The previous attempt had an uncertain outcome. Check the phone screen before sending again, then acknowledge the device state."
                .to_string(),
        ),
    }
}

fn cancel_requested(store: &CancelStore, serial: &str) -> bool {
    store.set.lock().unwrap().contains(serial)
}

fn clear_cancel(store: &CancelStore, serial: &str) {
    store.set.lock().unwrap().remove(serial);
}

fn set_cancel(store: &CancelStore, serial: &str) {
    store.set.lock().unwrap().insert(serial.to_string());
}

fn resource_candidate(app: &tauri::AppHandle, names: &[&str]) -> Option<PathBuf> {
    if let Ok(dir) = app.path().resource_dir() {
        for name in names {
            let path = dir.join(name);
            if path.exists() {
                return Some(path);
            }
        }
    }

    let cwd = std::env::current_dir().ok()?;
    for name in names {
        let path = cwd.join(name);
        if path.exists() {
            return Some(path);
        }
    }

    None
}

fn adb_path(app: &tauri::AppHandle) -> (PathBuf, String) {
    if let Some(path) = resource_candidate(
        app,
        &[
            "adb/adb.exe",
            "adb.exe",
            "_up_/packaging/platform-tools/adb.exe",
            "platform-tools/adb.exe",
            "packaging/platform-tools/adb.exe",
        ],
    ) {
        return (path, "BUNDLED".to_string());
    }

    if let Ok(path) = std::env::var("EMERGENCY_SIMULATOR_ADB") {
        let candidate = PathBuf::from(path);
        if candidate.exists() {
            return (candidate, "ENVIRONMENT".to_string());
        }
    }

    (PathBuf::from("adb"), "PATH".to_string())
}

fn command_output(app: &tauri::AppHandle, args: &[&str]) -> Result<Output, String> {
    let (adb, _) = adb_path(app);
    let mut command = Command::new(adb);
    command.args(args);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }

    command
        .output()
        .map_err(|e| format!("ADB could not be started: {e}"))
}

fn output_text(output: &Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !stderr.trim().is_empty() {
        if !text.trim().is_empty() {
            text.push('\n');
        }
        text.push_str(stderr.trim());
    }

    text
}

fn adb_call(app: &tauri::AppHandle, args: &[&str]) -> Result<String, String> {
    let output = command_output(app, args)?;
    let text = output_text(&output);

    if !output.status.success() {
        return Err(text.trim().to_string());
    }

    Ok(text)
}

fn shell(app: &tauri::AppHandle, serial: &str, parts: &[&str]) -> Result<String, String> {
    let mut args = vec!["-s", serial, "shell"];
    args.extend_from_slice(parts);
    adb_call(app, &args)
}

fn getprop(app: &tauri::AppHandle, serial: &str, key: &str) -> String {
    shell(app, serial, &["getprop", key])
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Every package on the device that looks like a Cell Broadcast receiver, most likely first.
///
/// An OEM distribution may ship more than one: a Google/Mainline module (updated through the
/// store) alongside a vendor build (a Galaxy A35 could carry both). Which one is actually
/// handling alerts is not decidable from the package list, so the controller carries them in
/// preference order and lets the device decide.
fn cellbroadcast_candidates(app: &tauri::AppHandle, serial: &str) -> Vec<String> {
    let text = match shell(app, serial, &["pm", "list", "packages"]) {
        Ok(text) => text,
        Err(_) => return Vec::new(),
    };

    let mut packages = text.lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix("package:"))
        .filter(|package| package.to_ascii_lowercase().contains("cellbroadcast"))
        .map(str::to_string)
        .collect::<Vec<_>>();

    // Rank by how specifically the name identifies a receiver module, then alphabetically so the
    // order is stable across runs rather than dependent on `pm list` output order.
    packages.sort_by_key(|package| {
        let lower = package.to_ascii_lowercase();
        let rank = if lower.contains("cellbroadcastreceiver") {
            0
        } else if lower.contains("cellbroadcast") {
            1
        } else {
            2
        };
        (rank, lower)
    });
    packages.dedup();

    packages
}

fn is_root(app: &tauri::AppHandle, serial: &str, build_type: &str) -> bool {
    // Never ask production/user Samsung builds to restart adbd as root.
    if build_type.eq_ignore_ascii_case("user") {
        return false;
    }

    let _ = adb_call(app, &["-s", serial, "root"]);
    thread::sleep(Duration::from_millis(700));

    if shell(app, serial, &["id", "-u"])
        .map(|text| text.trim() == "0")
        .unwrap_or(false)
    {
        return true;
    }

    if build_type.eq_ignore_ascii_case("eng")
        || build_type.eq_ignore_ascii_case("userdebug")
    {
        return shell(app, serial, &["id"])
            .map(|text| text.contains("uid=0"))
            .unwrap_or(false);
    }

    false
}

fn parse_first_u64(text: &str, key: &str) -> Option<u64> {
    text.lines().find_map(|line| {
        line.trim().strip_prefix(key)
            .and_then(|rest| rest.trim().split_whitespace().next())
            .and_then(|value| value.parse::<u64>().ok())
    })
}

fn parse_battery(text: &str) -> Option<u8> {
    text.lines().find_map(|line| {
        let line = line.trim();
        line.strip_prefix("level:")
            .and_then(|value| value.trim().parse::<u8>().ok())
    })
}

fn parse_storage_gb(text: &str) -> Option<f32> {
    let line = text.lines().filter(|line| !line.trim().is_empty()).last()?;
    let fields = line.split_whitespace().collect::<Vec<_>>();
    let kb = fields.get(1)?.parse::<f64>().ok()?;
    Some((kb / 1024.0 / 1024.0 * 10.0).round() as f32 / 10.0)
}

fn query_device_specs(app: &tauri::AppHandle, serial: &str, model: &str) -> DeviceSpecs {
    let mem = shell(app, serial, &["cat", "/proc/meminfo"]).unwrap_or_default();
    let ram_gb = parse_first_u64(&mem, "MemTotal:")
        .map(|kb| (kb as f32 / 1024.0 / 1024.0 * 10.0).round() / 10.0);

    let storage = shell(app, serial, &["df", "-k", "/data"]).unwrap_or_default();
    let storage_gb = parse_storage_gb(&storage);

    let battery = shell(app, serial, &["dumpsys", "battery"]).unwrap_or_default();
    let battery_percent = parse_battery(&battery);

    let size = shell(app, serial, &["wm", "size"]).unwrap_or_default();
    let screen_resolution = size.lines()
        .find(|line| line.trim().starts_with("Physical size:"))
        .and_then(|line| line.split(':').nth(1))
        .map(|value| value.trim().to_string());

    let density = shell(app, serial, &["wm", "density"]).unwrap_or_default()
        .lines()
        .find(|line| line.trim().starts_with("Physical density:"))
        .and_then(|line| line.split(':').nth(1))
        .and_then(|value| value.trim().parse::<u32>().ok());

    let lower = model.to_ascii_lowercase();
    let a35 = lower.contains("sm-a356") || lower.contains("galaxy a35");
    let mut cpu = getprop(app, serial, "ro.soc.model");
    if cpu.is_empty() { cpu = getprop(app, serial, "ro.board.platform"); }

    let (announced, dimensions, weight_g) = if a35 {
        (
            Some("March 11, 2024".to_string()),
            Some("161.7 × 78.0 × 8.2 mm".to_string()),
            Some(209),
        )
    } else { (None, None, None) };

    let (memory_options, storage_options, display_profile, battery_capacity_mah) = if a35 {
        (
            Some("6 / 8 GB".to_string()),
            Some("128 / 256 GB".to_string()),
            Some("6.6″ FHD+ Super AMOLED · up to 120 Hz".to_string()),
            Some(5000),
        )
    } else {
        (None, None, None, None)
    };

    DeviceSpecs {
        cpu: (!cpu.is_empty()).then_some(cpu),
        ram_gb,
        storage_gb,
        battery_percent,
        screen_resolution,
        density,
        announced,
        dimensions,
        weight_g,
        memory_options,
        storage_options,
        display_profile,
        battery_capacity_mah,
    }
}

fn local_simulator_installed(app: &tauri::AppHandle, serial: &str) -> bool {
    shell(app, serial, &["pm", "path", LOCAL_SIMULATOR_PACKAGE])
        .map(|text| text.lines().any(|line| line.trim_start().starts_with("package:")))
        .unwrap_or(false)
}

fn local_simulator_apk_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    resource_candidate(
        app,
        &[
            "android/local-simulator.apk",
            "local-simulator.apk",
            "_up_/android/local-simulator/app/build/outputs/apk/debug/app-debug.apk",
            "android/local-simulator/app/build/outputs/apk/debug/app-debug.apk",
        ],
    )
}

fn install_local_simulator(app: &tauri::AppHandle, serial: &str) -> Result<String, String> {
    let apk = local_simulator_apk_path(app)
        .ok_or_else(|| "The bundled local Android simulator APK is missing from this build.".to_string())?;
    adb_call(app, &["-s", serial, "install", "-r", "-d", &apk.to_string_lossy()])?;
    let _ = shell(app, serial, &["pm", "grant", LOCAL_SIMULATOR_PACKAGE, "android.permission.POST_NOTIFICATIONS"]);
    let _ = shell(app, serial, &["appops", "set", LOCAL_SIMULATOR_PACKAGE, "USE_FULL_SCREEN_INTENT", "allow"]);
    if !local_simulator_installed(app, serial) {
        return Err("ADB reported a successful install, but the local simulator package is not present.".to_string());
    }
    Ok("Local Android alert simulator installed.".to_string())
}

/// The capability state that decides what the operator will actually see on the phone.
///
/// Reported separately from installation because they fail independently: a simulator can be
/// installed and still be unable to post a notification (Android 13+ POST_NOTIFICATIONS denied) or
/// unable to take over the screen (Android 14+ USE_FULL_SCREEN_INTENT denied). Both degrade the
/// result without making the send fail, so the desktop has to be able to say which happened
/// rather than reporting a flat success or a flat failure.
#[derive(Debug, Clone, Serialize, Default)]
pub struct SimulatorCapabilities {
    pub installed: bool,
    pub post_notifications: Option<bool>,
    pub full_screen_intent: Option<bool>,
    pub notifications_enabled: Option<bool>,
}

fn simulator_capabilities(app: &tauri::AppHandle, serial: &str) -> SimulatorCapabilities {
    let mut capabilities = SimulatorCapabilities {
        installed: local_simulator_installed(app, serial),
        ..Default::default()
    };

    if !capabilities.installed {
        return capabilities;
    }

    // POST_NOTIFICATIONS is only runtime-granted from Android 13 (API 33). Below that the
    // permission does not exist and reporting `false` would be a false alarm.
    let sdk = getprop(app, serial, "ro.build.version.sdk")
        .trim()
        .parse::<u32>()
        .ok();
    if sdk.is_some_and(|sdk| sdk >= 33) {
        capabilities.post_notifications = shell(
            app,
            serial,
            &["dumpsys", "package", LOCAL_SIMULATOR_PACKAGE],
        )
        .ok()
        .and_then(|dump| {
            dump.lines()
                .find(|line| line.contains("android.permission.POST_NOTIFICATIONS"))
                .map(|line| line.contains("granted=true"))
        });
    }

    // `appops get` reports the current mode for the op. On Android 14+ this defaults to `deny`
    // for apps that are not calling/alarm apps, which is exactly the condition that makes a
    // full-screen alert silently degrade into a heads-up notification.
    capabilities.full_screen_intent = shell(
        app,
        serial,
        &["appops", "get", LOCAL_SIMULATOR_PACKAGE, "USE_FULL_SCREEN_INTENT"],
    )
    .ok()
    .map(|text| {
        let text = text.to_ascii_lowercase();
        if text.contains("allow") {
            true
        } else if text.contains("deny") || text.contains("ignore") || text.contains("default") {
            false
        } else {
            // Op not present on this API level; treat as unrestricted.
            true
        }
    });

    capabilities.notifications_enabled = shell(
        app,
        serial,
        &["dumpsys", "notification", "--noredact"],
    )
    .ok()
    .map(|dump| !dump.contains(&format!("{LOCAL_SIMULATOR_PACKAGE}: banned")));

    capabilities
}

fn local_simulator_command_script(title: &str, body: &str, severity: &str, category: u32) -> String {
    format!(
        "am broadcast --receiver-foreground -a {action} -n {component} --es title {title} --es message {body} --es severity {severity} --es category {category}",
        action = sh_quote("com.tirodz.emergencysimulator.TRIGGER_ALERT"),
        component = sh_quote("com.tirodz.emergencysimulator/.AlertReceiver"),
        title = sh_quote(title),
        body = sh_quote(body),
        severity = sh_quote(severity),
        category = sh_quote(&category.to_string()),
    )
}

/// The delivery verdict, decided purely from the stage lines the Android companion emitted.
///
/// Split out from the logcat polling so the decision can be tested against captured device output.
/// The decision is the part of this pipeline that must never be wrong: it is the difference
/// between telling the operator the alert appeared and telling them it did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalVerdict {
    pub state: String,
    pub message: String,
    pub failure: Option<String>,
    pub evidence: Vec<String>,
    pub terminal: bool,
}

fn has_stage(stages: &[(String, String)], name: &str) -> bool {
    stages.iter().any(|(found, _)| found == name)
}

fn stage_detail(stages: &[(String, String)], name: &str) -> Option<String> {
    stages
        .iter()
        .find(|(found, _)| found == name)
        .map(|(_, detail)| detail.clone())
        .filter(|detail| !detail.is_empty())
}

/// Decide what happened from the stages seen so far. `None` means "not conclusive yet, keep
/// polling" rather than "failed" -- an empty logcat is not evidence of a blocked alert.
fn local_simulator_verdict(stages: &[(String, String)]) -> Option<LocalVerdict> {
    // A receiver that ran and then failed to post is a definite failure, and reporting it beats
    // waiting out the timeout.
    if has_stage(stages, "NOTIFICATION_FAILED") {
        let detail = stage_detail(stages, "NOTIFICATION_FAILED").unwrap_or_default();
        return Some(LocalVerdict {
            state: "FAILED".to_string(),
            message: format!("The Android companion could not post the notification: {detail}"),
            failure: Some("NOTIFICATION_FAILED".to_string()),
            evidence: vec!["ANDROID_RECEIVER_ACCEPTED".to_string()],
            terminal: true,
        });
    }

    if !has_stage(stages, "NOTIFICATION_POSTED") {
        return None;
    }

    let full_screen = has_stage(stages, "FULLSCREEN_ACTIVITY_STARTED");
    let audio = has_stage(stages, "AUDIO_START");
    let vibration = has_stage(stages, "VIBRATION_START");

    let mut evidence = vec!["ANDROID_RECEIVER_ACCEPTED".to_string(), "NOTIFICATION_POSTED".to_string()];
    if full_screen {
        evidence.push("FULLSCREEN_ACTIVITY_STARTED".to_string());
    }
    if audio {
        evidence.push("AUDIO_START".to_string());
    }
    if vibration {
        evidence.push("VIBRATION_START".to_string());
    }

    if full_screen {
        Some(LocalVerdict {
            state: "ALERT_DISPLAYED".to_string(),
            message: format!(
                "Local alert confirmed on the device: notification posted, full-screen activity started{}{}.",
                if audio { ", audio started" } else { "" },
                if vibration { ", vibration started" } else { "" },
            ),
            failure: None,
            evidence,
            terminal: true,
        })
    } else {
        // Posted but not full-screen. Either Android withheld full-screen intent access or the
        // activity has not launched yet; both are honest partial results, not failures.
        let message = if has_stage(stages, "FULLSCREEN_ACTIVITY_UNAVAILABLE") {
            "Local notification posted with sound and vibration. Android did not grant full-screen intent access, so the alert did not take over the screen; the notification is tappable to open it."
        } else {
            "Local notification posted with sound and vibration. The full-screen activity was not observed; the notification is tappable to open the alert."
        };
        Some(LocalVerdict {
            state: "NOTIFICATION_POSTED".to_string(),
            message: message.to_string(),
            failure: None,
            evidence,
            terminal: true,
        })
    }
}

/// How long to wait for the companion's stages before giving up.
///
/// Measured on an unaccelerated Android 35 emulator, a single send took 6.5 s from broadcast to
/// `NOTIFICATION_POSTED` and a further 8 s before Android launched the full-screen activity -- 12.7 s
/// end to end, entirely from platform latency rather than from this code. An 8 s budget reported a
/// false `LOCAL_UI_EVIDENCE_TIMEOUT` for an alert that had in fact appeared, which is the exact
/// false-negative this project exists to prevent. The poll returns as soon as a terminal stage is
/// seen, so this ceiling only costs time on a device that is genuinely not responding.
const LOCAL_EVIDENCE_TIMEOUT: Duration = Duration::from_secs(25);

/// How long to keep polling after `NOTIFICATION_POSTED` before settling for a notification-only
/// verdict.
///
/// A notification and its full-screen activity do not arrive together: Android posts the
/// notification first and launches the activity afterwards, measured here at 8 s apart on an
/// unaccelerated emulator. Concluding at the first conclusive stage would therefore report
/// "notification only" for an alert that did take over the screen a moment later -- understating a
/// real success. This grace window lets the full-screen stage land before the verdict is fixed.
const FULLSCREEN_GRACE: Duration = Duration::from_secs(14);

/// Read downstream evidence and decide what actually happened on the device.
///
/// The decision is made from the Android companion's own stage lines, never from the broadcast
/// command's exit status. `am broadcast` printing "Broadcast completed" only means the intent was
/// delivered to a receiver that did not throw; it says nothing about a notification appearing, a
/// full-screen activity launching, or sound being audible.
fn collect_local_simulator_evidence(
    app: &tauri::AppHandle,
    serial: &str,
    cancel: &CancelStore,
    result: &mut SendResult,
    tx: &TxStore,
) -> Result<(), String> {
    let start = Instant::now();
    let mut stages: Vec<(String, String)> = Vec::new();
    let mut posted_at: Option<Instant> = None;

    while start.elapsed() < LOCAL_EVIDENCE_TIMEOUT {
        if cancel_requested(cancel, serial) {
            result.state = "CANCELLED".to_string();
            fail_stage(
                app,
                result,
                stage::TEST_FAILED,
                "USER_CANCELLED",
                "cancel requested",
                "Stop requested while waiting for local simulator evidence.",
                None,
                None,
            );
            tx.map.lock().unwrap().insert(serial.to_string(), TxState::Uncertain);
            return Ok(());
        }

        thread::sleep(Duration::from_millis(750));

        // Filtered by the companion's own tag. An unfiltered dump on a busy device can push the
        // early stages out of the retained window before the later ones arrive, which would look
        // exactly like an alert that never appeared.
        let dump = shell(
            app,
            serial,
            &["logcat", "-d", "-t", "4000", "-s", "EmergencySimulator:I"],
        )
        .unwrap_or_default();
        let seen = parse_android_stages(&dump);
        if !seen.is_empty() {
            stages = seen;
        }

        let Some(verdict) = local_simulator_verdict(&stages) else {
            continue;
        };

        // Keep waiting only while a full-screen stage could still arrive. A failure, or an alert
        // already confirmed full-screen, is final.
        if verdict.failure.is_none() && verdict.state != "ALERT_DISPLAYED" {
            let posted = *posted_at.get_or_insert_with(Instant::now);
            if posted.elapsed() < FULLSCREEN_GRACE {
                continue;
            }
        }

        if has_stage(&stages, "ANDROID_RECEIVER_ACCEPTED") {
            diag(
                app,
                result,
                "ANDROID_RECEIVER_ACCEPTED",
                "AlertReceiver.onReceive",
                stage_detail(&stages, "ANDROID_RECEIVER_ACCEPTED"),
                None,
                None,
                "ok",
            );
        }

        if verdict.failure.is_some() {
            fail_stage(
                app,
                result,
                stage::TEST_FAILED,
                verdict.failure.as_deref().unwrap_or("UNKNOWN"),
                "AlertNotificationHelper.show",
                verdict.message,
                None,
                None,
            );
            tx.map.lock().unwrap().insert(serial.to_string(), TxState::Uncertain);
            return Ok(());
        }

        for label in &verdict.evidence {
            if !result.evidence.iter().any(|existing| existing == label) {
                result.evidence.push(label.clone());
            }
        }

        diag(
            app,
            result,
            if verdict.state == "ALERT_DISPLAYED" {
                "FULLSCREEN_ACTIVITY_STARTED"
            } else {
                "NOTIFICATION_POSTED"
            },
            "delivery verdict",
            Some(verdict.message.clone()),
            None,
            None,
            if verdict.state == "ALERT_DISPLAYED" { "ok" } else { "warn" },
        );

        result.state = verdict.state.clone();
        result.message = verdict.message.clone();

        diag(
            app,
            result,
            stage::TEST_COMPLETE,
            "delivery verdict",
            Some(result.state.clone()),
            None,
            None,
            if result.state == "ALERT_DISPLAYED" { "ok" } else { "warn" },
        );

        tx.map.lock().unwrap().insert(serial.to_string(), TxState::Delivered);
        return Ok(());
    }

    // Nothing conclusive arrived. This is reported as uncertain rather than as a failure, because
    // an absent log line cannot distinguish "Android blocked it" from "the operator was not looking
    // at the phone" or "logcat rotated".
    result.state = "RECEIVED_BY_LOCAL_SIMULATOR".to_string();
    fail_stage(
        app,
        result,
        stage::TEST_FAILED,
        "LOCAL_UI_EVIDENCE_TIMEOUT",
        "logcat evidence",
        "The local simulator did not report any pipeline stage before timeout. Check that the phone is awake, that notifications are allowed for Emergency Simulator Local, and retry.",
        None,
        None,
    );
    tx.map.lock().unwrap().insert(serial.to_string(), TxState::Uncertain);
    Ok(())
}

fn parse_devices(app: &tauri::AppHandle) -> Result<Vec<Device>, String> {
    let _ = adb_call(app, &["start-server"]);
    let mut text = adb_call(app, &["devices", "-l"])?;
    if !text.lines().any(|line| {
        let line = line.trim();
        !line.is_empty() && !line.starts_with("*") && !line.starts_with("List of devices")
    }) {
        thread::sleep(Duration::from_millis(300));
        text = adb_call(app, &["devices", "-l"])?;
    }
    let mut devices = Vec::new();

    for line in text.lines().skip(1) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('*') {
            continue;
        }

        let mut fields = trimmed.split_whitespace();
        let serial = match fields.next() {
            Some(value) => value.to_string(),
            None => continue,
        };
        let raw_state = fields.next().unwrap_or("unknown");

        if raw_state == "unauthorized" {
            devices.push(Device {
                serial,
                model: None,
                product: None,
                manufacturer: None,
                release: None,
                sdk: None,
                build_type: None,
                debuggable: None,
                root: false,
                cellbroadcast_package: None,
                cellbroadcast_candidates: Vec::new(),
                local_simulator: false,
                state: DeviceState::Unauthorized,
                support_level: SupportLevel::Untested,
                specs: DeviceSpecs { cpu: None, ram_gb: None, storage_gb: None, battery_percent: None, screen_resolution: None, density: None, announced: None, dimensions: None, weight_g: None, memory_options: None, storage_options: None, display_profile: None, battery_capacity_mah: None },
                notes: vec![
                    "Accept the USB debugging authorization prompt on the phone."
                        .to_string(),
                ],
            });
            continue;
        }

        if raw_state == "offline" || raw_state == "no" {
            devices.push(Device {
                serial,
                model: None,
                product: None,
                manufacturer: None,
                release: None,
                sdk: None,
                build_type: None,
                debuggable: None,
                root: false,
                cellbroadcast_package: None,
                cellbroadcast_candidates: Vec::new(),
                local_simulator: false,
                state: DeviceState::Offline,
                support_level: SupportLevel::Untested,
                specs: DeviceSpecs { cpu: None, ram_gb: None, storage_gb: None, battery_percent: None, screen_resolution: None, density: None, announced: None, dimensions: None, weight_g: None, memory_options: None, storage_options: None, display_profile: None, battery_capacity_mah: None },
                notes: vec!["ADB reports the device as offline.".to_string()],
            });
            continue;
        }

        if raw_state != "device" {
            devices.push(Device {
                serial,
                model: None,
                product: None,
                manufacturer: None,
                release: None,
                sdk: None,
                build_type: None,
                debuggable: None,
                root: false,
                cellbroadcast_package: None,
                cellbroadcast_candidates: Vec::new(),
                local_simulator: false,
                state: DeviceState::Unknown,
                support_level: SupportLevel::Untested,
                specs: DeviceSpecs { cpu: None, ram_gb: None, storage_gb: None, battery_percent: None, screen_resolution: None, density: None, announced: None, dimensions: None, weight_g: None, memory_options: None, storage_options: None, display_profile: None, battery_capacity_mah: None },
                notes: vec![format!("ADB state: {raw_state}")],
            });
            continue;
        }

        let model = getprop(app, &serial, "ro.product.model");
        let product = getprop(app, &serial, "ro.product.device");
        let manufacturer = getprop(app, &serial, "ro.product.manufacturer");
        let release = getprop(app, &serial, "ro.build.version.release");
        let sdk = getprop(app, &serial, "ro.build.version.sdk");
        let build_type = getprop(app, &serial, "ro.build.type");
        let debuggable = getprop(app, &serial, "ro.debuggable");

        let root = is_root(app, &serial, &build_type);
        let cellbroadcast_candidates = cellbroadcast_candidates(app, &serial);
        let cellbroadcast_package = cellbroadcast_candidates.first().cloned();
        let local_simulator = local_simulator_installed(app, &serial);

        let samsung_a35 = model.to_ascii_lowercase().contains("sm-a356")
            || model.to_ascii_lowercase().contains("galaxy a35");

        let mut notes = Vec::new();

        if samsung_a35 {
            notes.push(
                "Galaxy A35 detected. Stock firmware is handled with read-only ADB diagnostics."
                    .to_string(),
            );
        }

        let specs = query_device_specs(app, &serial, &model);
        let (state, support_level) = if local_simulator {
            notes.push("Root-free local simulator is installed. Send uses our explicit test receiver and notification/full-screen pipeline.".to_string());
            (DeviceState::SimulatorReady, SupportLevel::LocalSimulator)
        } else if root && cellbroadcast_package.is_some() {
            notes.push(
                "Rooted/userdebug controlled target. The genuine Android CellBroadcast test path is available."
                    .to_string(),
            );
            (DeviceState::Ready, SupportLevel::Supported)
        } else if cellbroadcast_package.is_none() {
            notes.push("No CellBroadcast receiver package was detected. Install the local simulator to test alert UI on a stock device.".to_string());
            (DeviceState::Unsupported, SupportLevel::Unsupported)
        } else {
            notes.push(
                "Stock/non-root device. Install the bundled local simulator to test alert UI without root."
                    .to_string(),
            );
            (DeviceState::NoRoot, SupportLevel::RootRequired)
        };

        devices.push(Device {
            serial,
            model: (!model.is_empty()).then_some(model),
            product: (!product.is_empty()).then_some(product),
            manufacturer: (!manufacturer.is_empty()).then_some(manufacturer),
            release: (!release.is_empty()).then_some(release),
            sdk: (!sdk.is_empty()).then_some(sdk),
            build_type: (!build_type.is_empty()).then_some(build_type),
            debuggable: (!debuggable.is_empty()).then_some(debuggable),
            root,
            cellbroadcast_package,
            cellbroadcast_candidates,
            local_simulator,
            state,
            support_level,
            specs,
            notes,
        });
    }

    Ok(devices)
}

fn receiver_prefs_path(package: &str) -> String {
    format!(
        "/data/user_de/0/{package}/shared_prefs/{package}_preferences.xml"
    )
}

fn read_receiver_prefs(
    app: &tauri::AppHandle,
    serial: &str,
    package: &str,
) -> Result<String, String> {
    let path = receiver_prefs_path(package);
    shell(app, serial, &["cat", &path]).or_else(|_| Ok(String::new()))
}

fn flag_enabled(xml: &str, name: &str) -> bool {
    let needle = format!("name=\"{}\" value=\"true\"", name);
    xml.contains(&needle)
}

fn update_flag(xml: &str, name: &str) -> String {
    let needle = format!("name=\"{}\"", name);

    if let Some(position) = xml.find(&needle) {
        let end_offset = xml[position..].find('>').unwrap_or(0);
        let end = position + end_offset;

        let mut updated = xml.to_string();
        let segment = updated[position..=end].to_string();

        if let Some(value_start) = segment.find("value=\"") {
            let absolute = position + value_start + "value=\"".len();
            if let Some(value_end) = updated[absolute..].find('"') {
                updated.replace_range(absolute..absolute + value_end, "true");
            }
        } else {
            let insert_position =
                if end > position && updated.as_bytes()[end - 1] == b'/' {
                    end - 1
                } else {
                    end
                };
            updated.insert_str(insert_position, " value=\"true\"");
        }

        return updated;
    }

    if let Some(position) = xml.rfind("</map>") {
        let mut updated = xml.to_string();
        updated.insert_str(
            position,
            &format!(
                "  <boolean name=\"{}\" value=\"true\" />\n",
                name
            ),
        );
        return updated;
    }

    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\" standalone=\"yes\" ?>\n<map>\n  <boolean name=\"{}\" value=\"true\" />\n</map>\n",
        name
    )
}

fn push_text_file(
    app: &tauri::AppHandle,
    serial: &str,
    remote: &str,
    text: &str,
) -> Result<(), String> {
    let safe_serial: String = serial
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect();

    let local = std::env::temp_dir()
        .join(format!("emergency-sim-{safe_serial}.xml"));

    fs::write(&local, text)
        .map_err(|e| format!("Could not create temporary preferences: {e}"))?;

    let local_string = local.to_string_lossy().to_string();
    let pushed = adb_call(
        app,
        &[
            "-s",
            serial,
            "push",
            &local_string,
            "/data/local/tmp/emergency-sim-prefs.xml",
        ],
    );

    let _ = fs::remove_file(&local);
    pushed?;

    // Array-based: no intermediate shell string is constructed, so nothing in the path can be
    // reinterpreted. Quoting the remote path additionally tolerates a package name that somehow
    // reached this point with a metacharacter in it.
    let script = format!(
        "cat /data/local/tmp/emergency-sim-prefs.xml > {remote}; rm -f /data/local/tmp/emergency-sim-prefs.xml",
        remote = sh_quote(remote),
    );

    shell(app, serial, &["sh", "-c", &script]).map(|_| ())
}

fn prepare_test_mode(
    app: &tauri::AppHandle,
    serial: &str,
    package: &str,
    dry_run: bool,
    log: &impl Fn(String),
) -> Result<bool, String> {
    let path = receiver_prefs_path(package);
    let current = read_receiver_prefs(app, serial, package)?;

    let testing = flag_enabled(&current, "testing_mode");
    let enabled = flag_enabled(&current, "enable_test_alerts");

    if testing && enabled {
        log("CellBroadcast test mode already enabled".to_string());
        return Ok(true);
    }

    if dry_run {
        log("Dry run: controlled test preferences would be enabled.".to_string());
        return Ok(true);
    }

    let mut updated = current.clone();
    updated = update_flag(&updated, "testing_mode");
    updated = update_flag(&updated, "enable_test_alerts");

    log(format!("Preparing controlled test preferences at {path}"));
    push_text_file(app, serial, &path, &updated)?;

    let _ = shell(app, serial, &["am", "force-stop", package]);
    thread::sleep(Duration::from_secs(2));

    let verify = read_receiver_prefs(app, serial, package)?;

    Ok(
        flag_enabled(&verify, "testing_mode")
            && flag_enabled(&verify, "enable_test_alerts"),
    )
}

fn injector_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    resource_candidate(
        app,
        &[
            "android/alertinject.jar",
            "alertinject.jar",
            "_up_/android/alertinject/out/alertinject.jar",
            "android/alertinject/out/alertinject.jar",
            "alertinject/alertinject.jar",
        ],
    )
}

/// Quote one argument for the POSIX shell that runs on the device.
///
/// `adb shell` does not preserve an argument vector: it concatenates argv into a single string
/// that `/system/bin/sh` on the device re-parses. An argument containing a space, a quote, `$`,
/// `;`, `|`, `&`, `(`, `)`, `<`, `>` or a backtick therefore either splits into several words or
/// is interpreted by that shell. The single canonical defence is to wrap the value in single
/// quotes and encode any embedded single quote as `'\''`, which the quoting here does, in Rust
/// rather than on the device.
///
/// The `'` sequence is the only one no shell can misinterpret: inside single quotes every other
/// metacharacter is a literal, so once the embedded quotes are broken out there is nothing left
/// for the device shell to act on.
fn sh_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for character in value.chars() {
        if character == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(character);
        }
    }
    quoted.push('\'');
    quoted
}

/// The complete `app_process` invocation, quoted so the device shell receives exactly the three
/// arguments the injector expects regardless of what the operator typed into the body.
fn injector_command_script(package: &str, body: &str) -> String {
    format!(
        "CLASSPATH={class} app_process /system/bin {main} {category} {package} {body}",
        class = INJECTOR_REMOTE,
        main = sh_quote(INJECTOR_CLASS),
        category = sh_quote(&SERVICE_CATEGORY.to_string()),
        package = sh_quote(package),
        body = sh_quote(body),
    )
}

fn push_injector(
    app: &tauri::AppHandle,
    serial: &str,
) -> Result<(), String> {
    let jar = injector_path(app)
        .ok_or_else(|| "The bundled Android injector is missing from this build.".to_string())?;

    adb_call(
        app,
        &[
            "-s",
            serial,
            "push",
            &jar.to_string_lossy(),
            INJECTOR_REMOTE,
        ],
    )
    .map(|_| ())
}

fn collect_evidence(
    app: &tauri::AppHandle,
    serial: &str,
    cancel: &CancelStore,
    result: &mut SendResult,
    tx: &TxStore,
) -> Result<(), String> {
    let start = Instant::now();

    let mut receiver_seen = false;
    let mut service_seen = false;
    let mut audio_seen = false;
    let mut dialog_seen = false;

    while start.elapsed() < Duration::from_secs(EVIDENCE_TIMEOUT_SECS) {
        if cancel_requested(cancel, serial) {
            result.state = "CANCELLED".to_string();
            result.failure = Some("USER_CANCELLED".to_string());
            result.message =
                "Stop requested while waiting for downstream evidence. Outcome is treated as uncertain."
                    .to_string();
            tx.map
                .lock()
                .unwrap()
                .insert(serial.to_string(), TxState::Uncertain);
            return Ok(());
        }

        // The rejection and filtering signals are read *before* the sleep as well as after it.
        // Reading only after the sleep would add two seconds to every candidate fallback, and the
        // rejection is already present in the buffer the moment the broadcast is refused.
        let dump = shell(app, serial, &["logcat", "-d", "-t", "3000"])
            .unwrap_or_default();

        if dump.contains("CellBroadcastReceiver") && !receiver_seen {
            receiver_seen = true;
            result
                .evidence
                .push("CellBroadcastReceiver.onReceive".to_string());
        }

        if dump.contains("CBAlertService") && !service_seen {
            service_seen = true;
            result
                .evidence
                .push("CellBroadcastAlertService.onStartCommand".to_string());
        }

        if dump.contains("CellBroadcastAlertAudio") && !audio_seen {
            audio_seen = true;
            result.evidence.push("CellBroadcastAlertAudio".to_string());
        }

        if dump.contains("CellBroadcastAlertDialog") && !dialog_seen {
            dialog_seen = true;
            result
                .evidence
                .push("CellBroadcastAlertDialog".to_string());
        }

        if dump.contains("Permission Denial: not allowed to send broadcast") {
            result.state = "FAILED".to_string();
            result.failure = Some("BROADCAST_REJECTED".to_string());
            result.message =
                "Android rejected the protected broadcast before the system alert UI."
                    .to_string();
            return Ok(());
        }

        if dump.contains("ignoring alert of type")
            || dump.contains("received undefined channels")
        {
            result.state = "FAILED".to_string();
            result.failure = Some("CELLBROADCAST_FILTERED".to_string());
            result.message =
                "The CellBroadcast receiver explicitly filtered the test message."
                    .to_string();
            return Ok(());
        }

        if dialog_seen {
            result.state = "ALERT_DISPLAYED".to_string();
            result.message =
                "Genuine Android CellBroadcast alert UI was displayed on the device."
                    .to_string();
            tx.map
                .lock()
                .unwrap()
                .insert(serial.to_string(), TxState::Delivered);
            return Ok(());
        }

        thread::sleep(Duration::from_secs(2));
    }

    if service_seen {
        result.state = "RECEIVED_BY_CELLBROADCAST".to_string();
        result.failure = Some("TIMEOUT".to_string());
        result.message =
            "The CellBroadcast service started, but no system alert UI was observed within the timeout."
                .to_string();
        tx.map
            .lock()
            .unwrap()
            .insert(serial.to_string(), TxState::Uncertain);
    } else if receiver_seen {
        result.state = "FAILED".to_string();
        result.failure = Some("ALERT_PROCESSING_FAILED".to_string());
        result.message =
            "The receiver saw the message, but no downstream alert service evidence appeared."
                .to_string();
        tx.map
            .lock()
            .unwrap()
            .insert(serial.to_string(), TxState::Uncertain);
    } else {
        result.state = "FAILED".to_string();
        result.failure = Some("TIMEOUT".to_string());
        result.message =
            "No downstream CellBroadcast evidence was observed. The outcome is uncertain."
                .to_string();
        tx.map
            .lock()
            .unwrap()
            .insert(serial.to_string(), TxState::Uncertain);
    }

    Ok(())
}

#[tauri::command]
fn app_info(app: tauri::AppHandle) -> Result<AppInfo, String> {
    let (_, source) = adb_path(&app);

    Ok(AppInfo {
        version: "1.0.0".to_string(),
        adb_source: source,
        injector: injector_path(&app).is_some(),
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct AdbDiagnostics {
    pub source: String,
    pub version: String,
    pub server: String,
    pub devices_raw: String,
}

fn valid_network_endpoint(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value.chars().all(|c| c.is_ascii_alphanumeric() || ".:-[]".contains(c))
}

#[tauri::command]
fn adb_pair(app: tauri::AppHandle, address: String, pairing_code: String) -> Result<String, String> {
    if !valid_network_endpoint(&address) {
        return Err("Invalid pairing address. Use the IP:port shown under Wireless debugging.".to_string());
    }
    if pairing_code.len() < 4 || pairing_code.len() > 16 || !pairing_code.chars().all(|c| c.is_ascii_digit()) {
        return Err("Invalid pairing code. Enter the numeric code shown on the phone.".to_string());
    }

    let (adb, _) = adb_path(&app);
    let mut child = Command::new(adb)
        .args(["pair", &address])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("ADB pair could not start: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(pairing_code.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .map_err(|e| format!("Could not send pairing code: {e}"))?;
    }

    let output = child.wait_with_output()
        .map_err(|e| format!("ADB pair failed: {e}"))?;
    let text = output_text(&output);
    if !output.status.success() {
        return Err(text.trim().to_string());
    }
    Ok(text.trim().to_string())
}

#[tauri::command]
fn adb_connect(app: tauri::AppHandle, address: String) -> Result<String, String> {
    if !valid_network_endpoint(&address) {
        return Err("Invalid device address. Use the IP:port shown under Wireless debugging.".to_string());
    }
    adb_call(&app, &["connect", &address])
}

#[tauri::command]
fn restart_adb_server(app: tauri::AppHandle) -> Result<String, String> {
    let _ = adb_call(&app, &["kill-server"]);
    thread::sleep(Duration::from_millis(250));
    adb_call(&app, &["start-server"])
}

#[tauri::command]
fn adb_diagnostics(app: tauri::AppHandle) -> Result<AdbDiagnostics, String> {
    let (path, source) = adb_path(&app);
    Ok(AdbDiagnostics {
        source: format!("{source} · {}", path.display()),
        version: command_output(&app, &["version"]).map(|out| output_text(&out)).unwrap_or_else(|e| e),
        server: adb_call(&app, &["start-server"]).unwrap_or_else(|e| e),
        devices_raw: adb_call(&app, &["devices", "-l"]).unwrap_or_else(|e| e),
    })
}

#[tauri::command]
fn list_devices(app: tauri::AppHandle) -> Result<Vec<Device>, String> {
    parse_devices(&app)
}

#[tauri::command]
fn install_local_simulator_command(app: tauri::AppHandle, serial: String) -> Result<String, String> {
    install_local_simulator(&app, &serial)
}

#[tauri::command]
fn open_android_settings(
    app: tauri::AppHandle,
    serial: String,
) -> Result<(), String> {
    adb_call(
        &app,
        &[
            "-s",
            &serial,
            "shell",
            "am",
            "start",
            "-a",
            "android.settings.DEVELOPMENT_SETTINGS",
        ],
    )
    .map(|_| ())
}

#[tauri::command]
fn acknowledge(
    app: tauri::AppHandle,
    state: State<SharedTx>,
    serial: String,
) -> Result<(), String> {
    set_tx(&app, &state, &serial, None)
}

#[tauri::command]
fn request_cancel(
    state: State<SharedCancel>,
    serial: String,
) -> Result<(), String> {
    set_cancel(&state, &serial);
    Ok(())
}

#[tauri::command]
async fn send_test_alert(
    app: tauri::AppHandle,
    tx: State<'_, SharedTx>,
    cancel: State<'_, SharedCancel>,
    serial: String,
    body: String,
    dry_run: bool,
) -> Result<SendResult, String> {
    let tx_store = tx.inner().clone();
    let cancel_store = cancel.inner().clone();

    tauri::async_runtime::spawn_blocking(move || {
        let normalized_body = body.split_whitespace().collect::<Vec<_>>().join(" ");
        let normalized_body = if normalized_body.is_empty() {
            DEFAULT_BODY.to_string()
        } else {
            normalized_body
        };

        let mut result = SendResult {
            device_serial: serial.clone(),
            category: SERVICE_CATEGORY,
            body: normalized_body.clone(),
            state: "FAILED".to_string(),
            failure: None,
            message: String::new(),
            evidence: Vec::new(),
            injector_exit_code: None,
            diagnostics: Vec::new(),
            failed_stage: None,
        };

        diag(
            &app,
            &mut result,
            stage::TEST_CREATED,
            "send_test_alert",
            Some(format!(
                "category={SERVICE_CATEGORY} chars={} dry_run={dry_run}",
                normalized_body.len()
            )),
            None,
            None,
            "info",
        );

        if !normalized_body.starts_with(REQUIRED_PREFIX) {
            fail_stage(
                &app,
                &mut result,
                stage::TEST_FAILED,
                "INVALID_BODY",
                "validate body",
                "The message must begin with TEST.",
                None,
                None,
            );
            return Ok(result);
        }

        if normalized_body.len() > 300 {
            fail_stage(
                &app,
                &mut result,
                stage::TEST_FAILED,
                "INVALID_BODY",
                "validate body",
                "The message is longer than 300 characters.",
                None,
                None,
            );
            return Ok(result);
        }

        if let Some(reason) = gate_for(&tx_store, &serial) {
            fail_stage(
                &app,
                &mut result,
                stage::TEST_FAILED,
                "DUPLICATE_SEND_BLOCKED",
                "safety gate",
                reason,
                None,
                None,
            );
            return Ok(result);
        }

        diag(
            &app,
            &mut result,
            stage::DEVICE_SELECTED,
            "parse_devices",
            Some(format!("serial={serial}")),
            None,
            None,
            "info",
        );

        let mut device = parse_devices(&app)?
            .into_iter()
            .find(|device| device.serial == serial)
            .ok_or_else(|| "The selected device is no longer attached.".to_string())?;

        diag(
            &app,
            &mut result,
            stage::DEVICE_CHECK,
            "adb device state",
            Some(format!(
                "model={} android={} state={:?} root={} local_simulator={}",
                device.model.clone().unwrap_or_else(|| "unknown".to_string()),
                device.release.clone().unwrap_or_else(|| "?".to_string()),
                device.state,
                device.root,
                device.local_simulator,
            )),
            None,
            None,
            "info",
        );

        if dry_run {
            result.state = "READY_TO_SEND".to_string();
            result.message = match device.state {
                DeviceState::Unauthorized => {
                    "ADB target exists, but USB debugging authorization is still pending. No device changes were made.".to_string()
                }
                DeviceState::Offline => {
                    "ADB target is offline. No device changes were made.".to_string()
                }
                DeviceState::NoRoot => {
                    "Galaxy/stock device detected. Read-only diagnostics passed; the controlled protected alert injector is unavailable on this production build. No device changes were made.".to_string()
                }
                DeviceState::Unsupported => {
                    "ADB target inspected, but no CellBroadcast receiver package was found. No device changes were made.".to_string()
                }
                DeviceState::Unknown => {
                    "ADB target inspected, but its state is unknown. No device changes were made.".to_string()
                }
                DeviceState::Ready => {
                    "Controlled target is ready. Dry run made no device changes.".to_string()
                }
                DeviceState::SimulatorReady => {
                    "Root-free local simulator is installed. Dry run made no device changes.".to_string()
                }
            };

            emit_log(
                &app,
                format!("Dry run complete for {serial}; no device changes"),
                "ok",
            );
            result.failed_stage = None;
            result.failure = None;
            return Ok(result);
        }

        if matches!(device.state, DeviceState::NoRoot | DeviceState::Unsupported) {
            diag(
                &app,
                &mut result,
                stage::LOCAL_SIMULATOR_CHECK,
                "pm path",
                Some("not installed; installing the bundled local simulator".to_string()),
                None,
                None,
                "info",
            );
            match install_local_simulator(&app, &serial) {
                Ok(message) => {
                    device.local_simulator = true;
                    device.state = DeviceState::SimulatorReady;
                    device.support_level = SupportLevel::LocalSimulator;
                    diag(
                        &app,
                        &mut result,
                        stage::LOCAL_SIMULATOR_INSTALL,
                        "adb install",
                        Some(message),
                        None,
                        None,
                        "ok",
                    );
                }
                Err(error) => {
                    let _ = set_tx(&app, &tx_store, &serial, None);
                    fail_stage(
                        &app,
                        &mut result,
                        stage::LOCAL_SIMULATOR_INSTALL,
                        "LOCAL_SIMULATOR_INSTALL_FAILED",
                        "adb install",
                        error,
                        None,
                        None,
                    );
                    return Ok(result);
                }
            }
        }

        if matches!(device.state, DeviceState::SimulatorReady) {
            let capabilities = simulator_capabilities(&app, &serial);
            diag(
                &app,
                &mut result,
                stage::CAPABILITY_CHECK,
                "simulator capability probe",
                Some(format!(
                    "installed={} post_notifications={} full_screen_intent={} notifications_enabled={}",
                    capabilities.installed,
                    capabilities
                        .post_notifications
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "n/a".to_string()),
                    capabilities
                        .full_screen_intent
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    capabilities
                        .notifications_enabled
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                )),
                None,
                None,
                if capabilities.installed { "info" } else { "warn" },
            );

            // A denied POST_NOTIFICATIONS means the receiver will run and Android will drop the
            // notification silently. Saying so up front is the difference between an operator
            // fixing a phone setting and an operator concluding the tool is broken.
            if capabilities.post_notifications == Some(false) {
                let _ = set_tx(&app, &tx_store, &serial, None);
                fail_stage(
                    &app,
                    &mut result,
                    stage::CAPABILITY_CHECK,
                    "NOTIFICATION_PERMISSION_DENIED",
                    "POST_NOTIFICATIONS",
                    "Notification permission is not granted for Emergency Simulator Local. Grant notifications for that app on the phone, then retry.",
                    None,
                    None,
                );
                return Ok(result);
            }

            let _ = adb_call(&app, &["-s", &serial, "logcat", "-c"]);

            let script = local_simulator_command_script(
                "EMERGENCY SIMULATOR TEST",
                &normalized_body,
                "TEST",
                SERVICE_CATEGORY,
            );

            diag(
                &app,
                &mut result,
                stage::ADB_BROADCAST_DISPATCH,
                format!("adb -s {serial} shell {script}"),
                Some("explicit component com.tirodz.emergencysimulator/.AlertReceiver".to_string()),
                None,
                None,
                "info",
            );

            let output = match command_output(&app, &["-s", &serial, "shell", &script]) {
                Ok(output) => output,
                Err(error) => {
                    let _ = set_tx(&app, &tx_store, &serial, None);
                    fail_stage(
                        &app,
                        &mut result,
                        stage::ADB_BROADCAST_RESULT,
                        "ADB_TRANSPORT",
                        "adb shell am broadcast",
                        error,
                        None,
                        None,
                    );
                    return Ok(result);
                }
            };

            result.injector_exit_code = output.status.code();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let combined = output_text(&output).trim().to_string();

            diag(
                &app,
                &mut result,
                stage::ADB_BROADCAST_RESULT,
                "adb shell am broadcast",
                Some(format!("exit={:?}", output.status.code())),
                Some(stdout.clone()).filter(|s| !s.is_empty()),
                Some(stderr.clone()).filter(|s| !s.is_empty()),
                if output.status.success() { "info" } else { "error" },
            );

            // The exit status of `am broadcast` is deliberately not treated as delivery evidence.
            // It only means the command was accepted, so a failure here is reported and a success
            // still has to be proved by the stage lines below.
            if !output.status.success() {
                let _ = set_tx(&app, &tx_store, &serial, None);
                fail_stage(
                    &app,
                    &mut result,
                    stage::ADB_BROADCAST_RESULT,
                    "LOCAL_BROADCAST_FAILED",
                    "adb shell am broadcast",
                    if combined.is_empty() {
                        "The broadcast command was rejected and produced no output.".to_string()
                    } else {
                        combined
                    },
                    Some(stdout).filter(|s| !s.is_empty()),
                    Some(stderr).filter(|s| !s.is_empty()),
                );
                return Ok(result);
            }

            collect_local_simulator_evidence(
                &app,
                &serial,
                &cancel_store,
                &mut result,
                &tx_store,
            )?;
            clear_cancel(&cancel_store, &serial);
            let _ = persist_transactions(&app, &tx_store);
            return Ok(result);
        }

        if !matches!(device.state, DeviceState::Ready) {
            let (failure_code, message) = match device.state {
                DeviceState::Unauthorized => (
                    "DEVICE_UNAUTHORIZED",
                    "Accept the USB debugging authorization prompt on the phone first.",
                ),
                DeviceState::Offline => ("DEVICE_OFFLINE", "ADB reports this device as offline."),
                DeviceState::NoRoot => (
                    "NO_ROOT",
                    "This stock/non-root device cannot use the controlled protected test path.",
                ),
                DeviceState::Unsupported => (
                    "CELLBROADCAST_MISSING",
                    "No CellBroadcast receiver package was detected.",
                ),
                DeviceState::Unknown => (
                    "DEVICE_UNKNOWN",
                    "The device is not in a known ADB-ready state.",
                ),
                DeviceState::Ready => ("UNKNOWN", "Device is ready."),
                DeviceState::SimulatorReady => {
                    ("LOCAL_SIMULATOR", "Local Android simulator is ready.")
                }
            };

            fail_stage(
                &app,
                &mut result,
                stage::DEVICE_CHECK,
                failure_code,
                "device state gate",
                message,
                None,
                None,
            );
            return Ok(result);
        }

        let candidates = if device.cellbroadcast_candidates.is_empty() {
            device
                .cellbroadcast_package
                .clone()
                .into_iter()
                .collect::<Vec<_>>()
        } else {
            device.cellbroadcast_candidates.clone()
        };

        if candidates.is_empty() {
            fail_stage(
                &app,
                &mut result,
                stage::DEVICE_CHECK,
                "CELLBROADCAST_MISSING",
                "cellbroadcast discovery",
                "No CellBroadcast receiver package was detected.",
                None,
                None,
            );
            return Ok(result);
        }

        diag(
            &app,
            &mut result,
            stage::DEVICE_CHECK,
            "cellbroadcast candidates",
            Some(candidates.join(", ")),
            None,
            None,
            "ok",
        );

        let test_mode = prepare_test_mode(
            &app,
            &serial,
            &candidates[0],
            false,
            &|message| emit_log(&app, message, "info"),
        )?;

        if !test_mode {
            fail_stage(
                &app,
                &mut result,
                stage::TEST_FAILED,
                "TEST_MODE_DISABLED",
                "prepare_test_mode",
                "The controlled test-alert preferences could not be established.",
                None,
                None,
            );
            return Ok(result);
        }

        if cancel_requested(&cancel_store, &serial) {
            clear_cancel(&cancel_store, &serial);
            result.state = "CANCELLED".to_string();
            fail_stage(
                &app,
                &mut result,
                stage::TEST_FAILED,
                "USER_CANCELLED",
                "cancel requested",
                "Operation cancelled before delivery.",
                None,
                None,
            );
            return Ok(result);
        }

        set_tx(&app, &tx_store, &serial, Some(TxState::Busy))?;

        let _ = adb_call(&app, &["-s", &serial, "logcat", "-c"]);

        if let Err(error) = push_injector(&app, &serial) {
            let _ = set_tx(&app, &tx_store, &serial, None);
            fail_stage(
                &app,
                &mut result,
                stage::TEST_FAILED,
                "INJECTOR_FAILURE",
                "adb push alertinject.jar",
                error,
                None,
                None,
            );
            return Ok(result);
        }

        emit_log(&app, "Injector pushed to the controlled device", "ok");

        emit_log(
            &app,
            format!("Sending ETWS TEST; category {SERVICE_CATEGORY}"),
            "info",
        );

        // One pre-quoted string, passed as a single `adb shell` argument. Handing adb separate
        // argv elements would let it join them unquoted and let the device shell re-split the
        // body on whitespace and act on metacharacters.
        //
        // Each candidate is tried in the same, already-prepared test-mode context. The next
        // candidate is attempted *only* when the platform explicitly rejected the broadcast for
        // the one just tried -- never on a timeout or a plain absence of evidence, because that
        // would risk stacking a second alert on a device that may already be showing the first.
        let mut last_detail: Option<String> = None;

        for (index, package) in candidates.iter().enumerate() {
            if cancel_requested(&cancel_store, &serial) {
                clear_cancel(&cancel_store, &serial);
                result.state = "CANCELLED".to_string();
                fail_stage(
                    &app,
                    &mut result,
                    stage::TEST_FAILED,
                    "USER_CANCELLED",
                    "cancel requested",
                    "Operation cancelled before delivery.",
                    None,
                    None,
                );
                let _ = set_tx(&app, &tx_store, &serial, None);
                return Ok(result);
            }

            if index > 0 {
                emit_log(
                    &app,
                    format!("Retrying with alternate receiver: {package}"),
                    "warn",
                );
                let _ = adb_call(&app, &["-s", &serial, "logcat", "-c"]);
            }

            let script = injector_command_script(package, &normalized_body);

            diag(
                &app,
                &mut result,
                stage::ADB_BROADCAST_DISPATCH,
                format!("adb -s {serial} shell {script}"),
                Some(format!("cellbroadcast receiver {package}")),
                None,
                None,
                "info",
            );

            let output = match command_output(&app, &["-s", &serial, "shell", &script]) {
                Ok(output) => output,
                Err(error) => {
                    // adb could not be started at all, so no broadcast was sent. Clearing the
                    // gate lets the operator retry; leaving it Busy would lock the device out
                    // until the safety state was reset by hand.
                    let _ = set_tx(&app, &tx_store, &serial, None);
                    result.state = "FAILED".to_string();
                    fail_stage(
                        &app,
                        &mut result,
                        stage::ADB_BROADCAST_RESULT,
                        "ADB_TRANSPORT",
                        "adb shell app_process",
                        error,
                        None,
                        None,
                    );
                    clear_cancel(&cancel_store, &serial);
                    return Ok(result);
                }
            };

            result.injector_exit_code = output.status.code();

            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let injector_text = output_text(&output);

            diag(
                &app,
                &mut result,
                stage::ADB_BROADCAST_RESULT,
                "adb shell app_process",
                Some(format!("exit={:?}", output.status.code())),
                Some(stdout).filter(|s| !s.is_empty()),
                Some(stderr).filter(|s| !s.is_empty()),
                if output.status.success() { "info" } else { "error" },
            );

            // An injector that never ran as root cannot have produced an alert, and saying so is
            // more useful than reporting "no evidence". `app_process` exits 0 either way, so the
            // exit code alone cannot distinguish this; only the injector's own refusal text can.
            if !output.status.success() || injector_text.contains("Permission Denial") {
                let detail = injector_text
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("the injector produced no output")
                    .trim()
                    .to_string();

                let _ = set_tx(&app, &tx_store, &serial, None);
                result.state = "FAILED".to_string();
                fail_stage(
                    &app,
                    &mut result,
                    stage::ADB_BROADCAST_RESULT,
                    "INJECTOR_FAILURE",
                    "on-device injector identity",
                    format!("The on-device injector did not run as a system identity. {detail}"),
                    None,
                    None,
                );
                clear_cancel(&cancel_store, &serial);
                return Ok(result);
            }

            collect_evidence(
                &app,
                &serial,
                &cancel_store,
                &mut result,
                &tx_store,
            )?;

            if result.state == "CANCELLED" {
                clear_cancel(&cancel_store, &serial);
                return Ok(result);
            }

            let rejected = result.state == "FAILED"
                && result.failure.as_deref() == Some("BROADCAST_REJECTED");

            if !rejected {
                break;
            }

            last_detail = Some(format!(
                "Android rejected the protected broadcast for {package}."
            ));
            let _ = set_tx(&app, &tx_store, &serial, None);

            // A rejection produced no downstream evidence, so nothing is on screen and trying the
            // next candidate cannot stack an alert.
            if index + 1 >= candidates.len() {
                break;
            }

            result.evidence.clear();
            result.state = "FAILED".to_string();
            result.failure = Some("BROADCAST_REJECTED".to_string());
        }

        if result.state == "FAILED" && result.failure.as_deref() == Some("BROADCAST_REJECTED") {
            if let Some(detail) = last_detail {
                result.message = if candidates.len() > 1 {
                    format!("{detail} No alternate CellBroadcast receiver on this device accepted it.")
                } else {
                    detail
                };
            }
        }

        let _ = persist_transactions(&app, &tx_store);
        clear_cancel(&cancel_store, &serial);

        if result.state == "ALERT_DISPLAYED" {
            diag(
                &app,
                &mut result,
                stage::TEST_COMPLETE,
                "delivery verdict",
                Some("ALERT_DISPLAYED".to_string()),
                None,
                None,
                "ok",
            );
        } else if result.failure.is_none() {
            // Reached the end without a verdict and without an explicit failure. Recorded so an
            // unexplained outcome is never silently reported as a success.
            let unfinished = result.state.clone();
            fail_stage(
                &app,
                &mut result,
                stage::TEST_FAILED,
                "NO_VERDICT",
                "delivery verdict",
                format!("The pipeline finished in state {unfinished} without downstream evidence."),
                None,
                None,
            );
        }

        Ok(result)
    })
    .await
    .map_err(|error| format!("Controller worker failed: {error}"))?
}

#[tauri::command]
fn window_minimize(window: tauri::Window) -> Result<(), String> {
    window.minimize().map_err(|e| e.to_string())
}

#[tauri::command]
fn window_toggle_maximize(window: tauri::Window) -> Result<(), String> {
    if window.is_maximized().map_err(|e| e.to_string())? {
        window.unmaximize().map_err(|e| e.to_string())
    } else {
        window.maximize().map_err(|e| e.to_string())
    }
}

#[tauri::command]
fn window_close(window: tauri::Window) -> Result<(), String> {
    window.close().map_err(|e| e.to_string())
}

#[tauri::command]
fn reset_safety_state(
    app: tauri::AppHandle,
    state: State<SharedTx>,
) -> Result<(), String> {
    *state.error.lock().unwrap() = None;
    *state.map.lock().unwrap() = HashMap::new();
    let _ = fs::remove_file(tx_path(&app)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The defect this guards is the one BUG-001 recorded in the retired Python controller: a
    /// multi-word body arriving at the injector as several shell words, so only the first was
    /// bound to `argv[2]`. The alert still displayed and every exit code was 0, which is why the
    /// project treats exit codes as evidence of nothing.
    #[test]
    fn injector_command_survives_a_multi_word_body() {
        let script = injector_command_script(
            "com.google.android.cellbroadcastreceiver",
            "TEST ALERT - SIMULATION",
        );

        assert!(script.contains("'TEST ALERT - SIMULATION'"));
        assert!(!script.contains("TEST ALERT - SIMULATION "));
    }

    #[test]
    fn injector_command_quotes_every_injector_argument() {
        let script = injector_command_script(
            "com.samsung.android.cellbroadcastreceiver",
            "TEST ALERT - SIMULATION",
        );

        assert!(script.contains(&format!("'{}'", INJECTOR_CLASS)));
        assert!(script.contains(&format!("'{}'", SERVICE_CATEGORY)));
        assert!(script.contains("'com.samsung.android.cellbroadcastreceiver'"));
        assert!(script.starts_with("CLASSPATH=/data/local/tmp/alertinject.jar app_process /system/bin"));
    }

    #[test]
    fn sh_quote_neutralises_shell_metacharacters() {
        // If any of these reached the device shell unquoted, the command would be split or the
        // tail would be executed as a separate command.
        let hostile = "TEST; rm -rf /data/local/tmp | cat $(id) `whoami` & echo > /sdcard/x";
        let quoted = sh_quote(hostile);

        assert_eq!(quoted, format!("'{}'", hostile));
        assert_eq!(quoted.matches('\'').count(), 2);
        assert!(quoted.starts_with('\'') && quoted.ends_with('\''));
    }

    #[test]
    fn sh_quote_escapes_an_embedded_single_quote() {
        let quoted = sh_quote("TEST it's here");
        assert_eq!(quoted, "'TEST it'\\''s here'");

        // Reading the quoting back the way a POSIX shell would must reproduce the input exactly.
        assert_eq!(unquote_posix(&quoted), "TEST it's here");
    }

    #[test]
    fn sh_quote_round_trips_every_printable_ascii_body() {
        // Every byte the operator can type must come back unchanged, so no body can be silently
        // mangled into a different alert.
        for byte in 0x20u8..=0x7e {
            let original = format!("TEST {}", byte as char);
            let quoted = sh_quote(&original);
            assert_eq!(unquote_posix(&quoted), original, "failed for {byte:#04x}");
        }
    }

    #[test]
    fn sh_quote_round_trips_quotes_backslashes_and_newlines() {
        for original in [
            "TEST 'quoted'",
            "TEST \\ backslash \\\\",
            "TEST\nsecond line",
            "TEST \"double\" and 'single'",
            "TEST $HOME ${PATH} $(id)",
            "TEST 日本語 🚨",
        ] {
            assert_eq!(unquote_posix(&sh_quote(original)), original, "failed for {original:?}");
        }
    }

    #[test]
    fn sh_quote_leaves_an_already_safe_body_readable() {
        assert_eq!(sh_quote("TEST ALERT"), "'TEST ALERT'");
        assert_eq!(sh_quote(""), "''");
    }

    /// A minimal POSIX single-quote reader, used only to prove the quoting is lossless. It
    /// implements the one rule `sh_quote` relies on: `'\''` closes, emits a literal quote, and
    /// reopens.
    fn unquote_posix(quoted: &str) -> String {
        let mut out = String::new();
        let mut chars = quoted.chars().peekable();
        let mut in_quotes = false;

        while let Some(character) = chars.next() {
            match character {
                '\'' => in_quotes = !in_quotes,
                '\\' if !in_quotes => {
                    if let Some(next) = chars.next() {
                        out.push(next);
                    }
                }
                other => out.push(other),
            }
        }

        out
    }

    // ---------------------------------------------------------------------------------------
    // Stage parsing and the delivery verdict.
    //
    // The fixtures below are real logcat lines captured from a running Android 35 emulator with
    // the companion APK installed, not hand-written approximations of the format. That matters:
    // the format is the contract between the Kotlin companion and this controller, so a test that
    // invents its own version of it proves nothing.
    // ---------------------------------------------------------------------------------------

    /// Exactly what the companion wrote for a broadcast that posted a notification.
    const CAPTURED_NOTIFICATION_RUN: &str = concat!(
        "09-21 22:44:51.792  3657  3657 I EmergencySimulator: ",
        "EMSIM:STAGE=ANDROID_RECEIVER_ACCEPTED category=4355 severity=TEST chars=15\n",
        "09-21 22:44:55.122  3657  3657 I EmergencySimulator: ",
        "EMSIM:STAGE=NOTIFICATION_POSTED id=4355\n",
    );

    #[test]
    fn parses_stage_lines_from_real_device_logcat() {
        let stages = parse_android_stages(CAPTURED_NOTIFICATION_RUN);

        assert_eq!(
            stages,
            vec![
                (
                    "ANDROID_RECEIVER_ACCEPTED".to_string(),
                    "category=4355 severity=TEST chars=15".to_string()
                ),
                ("NOTIFICATION_POSTED".to_string(), "id=4355".to_string()),
            ]
        );
    }

    #[test]
    fn ignores_logcat_lines_that_are_not_stage_lines() {
        let noise = concat!(
            "09-21 22:43:56.894  1679  3321 E ActivityManager:  +0% 3657/com.tirodz.emergencysimulator\n",
            "09-21 22:44:00.168  1679  1793 D ActivityManager: freezing 3657 com.tirodz.emergencysimulator\n",
            "--------- beginning of main\n",
        );

        assert!(parse_android_stages(noise).is_empty());
    }

    #[test]
    fn a_posted_notification_without_full_screen_is_not_reported_as_displayed() {
        // This is the defect class the project exists to avoid: reporting a full-screen alert when
        // only a notification was posted. Android 14+ withholds full-screen intent access by
        // default, so this is the common outcome on a stock device and must not be upgraded.
        let stages = parse_android_stages(CAPTURED_NOTIFICATION_RUN);
        let verdict = local_simulator_verdict(&stages).expect("should be conclusive");

        assert_eq!(verdict.state, "NOTIFICATION_POSTED");
        assert_ne!(verdict.state, "ALERT_DISPLAYED");
        assert!(verdict.failure.is_none());
        assert_eq!(
            verdict.evidence,
            vec!["ANDROID_RECEIVER_ACCEPTED", "NOTIFICATION_POSTED"]
        );
    }

    #[test]
    fn full_screen_activity_upgrades_the_verdict_and_records_audio_and_vibration() {
        let stages = parse_android_stages(&format!(
            "{CAPTURED_NOTIFICATION_RUN}\
             EMSIM:STAGE=FULLSCREEN_ACTIVITY_STARTED fullScreenAllowed=true\n\
             EMSIM:STAGE=AUDIO_START usage=ALARM looping=true\n\
             EMSIM:STAGE=VIBRATION_START pattern=700\n"
        ));
        let verdict = local_simulator_verdict(&stages).expect("should be conclusive");

        assert_eq!(verdict.state, "ALERT_DISPLAYED");
        assert!(verdict.failure.is_none());
        assert!(verdict.evidence.contains(&"FULLSCREEN_ACTIVITY_STARTED".to_string()));
        assert!(verdict.evidence.contains(&"AUDIO_START".to_string()));
        assert!(verdict.evidence.contains(&"VIBRATION_START".to_string()));
        assert!(verdict.message.contains("audio started"));
    }

    #[test]
    fn an_empty_logcat_is_inconclusive_rather_than_a_failure() {
        // "Broadcast completed: result=0" with no downstream evidence must never be read as
        // success, but it must not be read as a definite failure either: the alert may simply not
        // have been observed. `None` keeps the collector polling.
        assert!(local_simulator_verdict(&[]).is_none());

        let receiver_only = parse_android_stages(
            "I EmergencySimulator: EMSIM:STAGE=ANDROID_RECEIVER_ACCEPTED category=4355\n",
        );
        assert!(local_simulator_verdict(&receiver_only).is_none());
    }

    #[test]
    fn a_failed_notification_is_terminal_and_named() {
        let stages = parse_android_stages(
            "I EmergencySimulator: EMSIM:STAGE=ANDROID_RECEIVER_ACCEPTED category=4355\n\
             I EmergencySimulator: EMSIM:STAGE=NOTIFICATION_FAILED SecurityException: not allowed\n",
        );
        let verdict = local_simulator_verdict(&stages).expect("should be conclusive");

        assert_eq!(verdict.state, "FAILED");
        assert_eq!(verdict.failure.as_deref(), Some("NOTIFICATION_FAILED"));
        assert!(verdict.message.contains("SecurityException"));
    }

    #[test]
    fn stage_names_used_by_the_verdict_match_the_android_contract() {
        // The Kotlin companion writes these names; the controller matches them. If a rename lands
        // on one side only, delivery detection silently stops working and every send would report
        // an evidence timeout, so the pairing is asserted here rather than trusted.
        let kotlin = include_str!(
            "../../android/local-simulator/app/src/main/java/com/tirodz/emergencysimulator/AlertStages.kt"
        );

        for name in [
            "ANDROID_RECEIVER_ACCEPTED",
            "NOTIFICATION_POSTED",
            "NOTIFICATION_FAILED",
            "FULLSCREEN_ACTIVITY_STARTED",
            "FULLSCREEN_ACTIVITY_UNAVAILABLE",
            "AUDIO_START",
            "VIBRATION_START",
            "USER_DISMISSED",
        ] {
            assert!(
                kotlin.contains(&format!("\"{name}\"")),
                "AlertStages.kt no longer declares {name}"
            );
            assert!(
                kotlin.contains("EMSIM:STAGE="),
                "AlertStages.kt no longer uses the EMSIM:STAGE= prefix the controller parses"
            );
        }
    }

    /// The receiver must never claim a full-screen alert it did not get. On Android 14+ the
    /// full-screen intent op defaults to denied, so "notification posted, screen not taken over"
    /// is the expected stock-device outcome and the code must not paper over it.
    #[test]
    fn notification_helper_reports_full_screen_capability_without_hidden_apis() {
        let helper = include_str!(
            "../../android/local-simulator/app/src/main/java/com/tirodz/emergencysimulator/AlertNotificationHelper.kt"
        );

        assert!(
            helper.contains("canUseFullScreenIntent()"),
            "the helper should consult the public full-screen intent API"
        );
        assert!(
            helper.contains("setFullScreenIntent"),
            "the helper should still request a full-screen intent"
        );

        // Checked against code only: the comment above deliberately names the hidden constant while
        // explaining why it is not used, and a naive substring search would flag that comment.
        let code: String = helper
            .lines()
            .filter(|line| {
                let trimmed = line.trim_start();
                !(trimmed.starts_with('*') || trimmed.starts_with("//") || trimmed.starts_with("/*"))
            })
            .collect::<Vec<_>>()
            .join("\n");

        // The hidden app-op constant is not in the SDK; referencing it fails the build.
        assert!(
            !code.contains("OPSTR_USE_FULL_SCREEN_INTENT"),
            "the helper must not depend on a hidden API constant"
        );
    }
}


pub fn run() {
    let transactions: SharedTx = Arc::new(TxStore::default());
    let cancellations: SharedCancel = Arc::new(CancelStore::default());
    tauri::Builder::default()
        .manage(transactions.clone())
        .manage(cancellations)
        .setup(move |app| {
            load_transactions(app.handle(), &transactions);
            emit_log(
                app.handle(),
                "Emergency Simulator desktop runtime started",
                "ok",
            );
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            adb_diagnostics,
            adb_pair,
            adb_connect,
            restart_adb_server,
            list_devices,
            install_local_simulator_command,
            open_android_settings,
            send_test_alert,
            acknowledge,
            request_cancel,
            reset_safety_state,
            window_minimize,
            window_toggle_maximize,
            window_close
        ])
        .run(tauri::generate_context!())
        .expect("error while running Emergency Simulator");
}
