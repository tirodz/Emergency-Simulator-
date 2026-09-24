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

mod diagnostics;
mod platform;

use platform::{CapabilityStage, ProbeEvidence, State as PlatformState};

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
    /// Whether the AOSP test entry point exists on this build, and why.
    pub test_entrypoint_available: PlatformState,
    pub test_entrypoint_reason: String,
    /// The strongest capability established by evidence rather than by inference.
    pub capability_stage: CapabilityStage,
    /// Which alert path this controller will take for this device, and what it may be called.
    /// The UI must label a `LOCAL UI SIMULATION` as a simulation, never as a Cell Broadcast.
    pub alert_mode: platform::AlertMode,
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
    /// Wall-clock duration of the whole send, in milliseconds. `None` for results that never
    /// reached the transport (validation failures).
    pub duration_ms: Option<u64>,
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
    /// How long the underlying command took, when it was timed. This is what separates "the
    /// command failed" from "the command never came back": without it a hang and an error read the
    /// same in the log.
    pub duration_ms: Option<u64>,
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
        duration_ms: None,
        timestamp: now_timestamp(),
    });
}

/// Record a diagnostic event for a command whose execution time was measured.
///
/// Kept alongside `diag` rather than folded into it so that a duration is only ever reported for
/// something that was actually timed. Stamping a duration on untimed events would invent data.
fn diag_timed(
    app: &tauri::AppHandle,
    result: &mut SendResult,
    stage: &str,
    action: impl Into<String>,
    detail: Option<String>,
    stdout: Option<String>,
    stderr: Option<String>,
    duration_ms: u64,
    kind: &str,
) {
    let action = action.into();
    let summary = match &detail {
        Some(detail) => format!("{stage} · {action} · {detail} · {duration_ms}ms"),
        None => format!("{stage} · {action} · {duration_ms}ms"),
    };
    emit_log(app, summary, kind);

    result.diagnostics.push(DiagEvent {
        stage: stage.to_string(),
        serial: result.device_serial.clone(),
        action,
        detail,
        stdout,
        stderr,
        duration_ms: Some(duration_ms),
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

/// Run a command and keep *everything*: exit code, stdout, stderr and the argv that produced it.
///
/// The diagnostic panel and the capability probe both need the unedited record. Collapsing stderr
/// into stdout or dropping a non-zero exit code is what makes a diagnostic tool unable to explain
/// its own conclusions, so this never discards either.
fn run_captured(app: &tauri::AppHandle, args: &[&str]) -> (String, ProbeEvidence) {
    let command = format!("adb {}", args.join(" "));
    let label = args.last().map(|s| s.to_string()).unwrap_or_default();

    match command_output(app, args) {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let code = output.status.code();
            (
                stdout.clone(),
                ProbeEvidence {
                    label,
                    command,
                    exit_code: code,
                    stdout,
                    stderr,
                    parsed: if output.status.success() {
                        String::new()
                    } else {
                        format!("command failed with exit code {}", code.unwrap_or(-1))
                    },
                },
            )
        }
        Err(error) => (
            String::new(),
            ProbeEvidence {
                label,
                command,
                exit_code: None,
                stdout: String::new(),
                stderr: error.clone(),
                parsed: error,
            },
        ),
    }
}

/// Trim long command output for the diagnostic panel without hiding that it was trimmed.
fn truncate_for_log(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= limit {
        return trimmed.to_string();
    }
    let kept: String = trimmed.chars().take(limit).collect();
    format!("{kept}\n…[{} more characters omitted]", trimmed.chars().count() - limit)
}

/// `shell` that also returns the raw evidence for the diagnostic panel.
fn shell_captured(
    app: &tauri::AppHandle,
    serial: &str,
    parts: &[&str],
) -> (String, ProbeEvidence) {
    let mut args = vec!["-s", serial, "shell"];
    args.extend_from_slice(parts);
    run_captured(app, &args)
}

fn getprop(app: &tauri::AppHandle, serial: &str, key: &str) -> String {
    shell(app, serial, &["getprop", key])
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Build the argv for the AOSP platform test-injection broadcast.
///
/// Split out from the send so the argument contract can be asserted without a device. Four details
/// are deliberate and were each wrong before (BUG-021):
///
/// * no `-n` — `GsmInboundSmsHandler` registers the test receiver dynamically, so it has no
///   manifest component name. `-n` also takes `package/class`, and the package names that
///   `cellbroadcast_candidates()` returns would be rejected as a bad component name.
/// * `--es pdu_string <hex>` — the key the handler reads.
/// * `--es format 3gpp` is not sent: `pdu_string` is already the encoded PDU and the handler does
///   not read a `format` key.
/// * No `--ei phone_id`. `CbTestBroadcastReceiver.onReceive` in `InboundSmsHandler` returns early
///   when `phone_id` is present and does not equal the handler's own phone id:
///
///   ```java
///   int phoneId = mPhone.getPhoneId();
///   if (intent.getIntExtra("phone_id", phoneId) != phoneId) {
///       return;
///   }
///   ```
///
///   A pinned `0` therefore fails **silently** on a device whose active subscription is not phone
///   0 — the receiver returns, `am` still exits 0, and the attempt looks identical to a build that
///   lacks the receiver. Omitting the extra uses the default `phoneId`, so the handler for whichever
///   phone is active accepts it. On a dual-SIM device both handlers may run, which is a visible,
///   diagnosable outcome rather than silence, so it is the safer default.
fn platform_test_alert_args<'a>(serial: &'a str, pdu_hex: &'a str) -> Vec<&'a str> {
    vec![
        "-s",
        serial,
        "shell",
        "am",
        "broadcast",
        "-a",
        platform::TEST_TRIGGER_ACTION,
        "--es",
        "pdu_string",
        pdu_hex,
    ]
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

/// Read whether the device is already running adbd as root, without changing anything.
///
/// This used to call `adb root` during device discovery. That is not a query: it restarts the adbd
/// daemon on the device as root, which is a change to the phone's state, and it ran on every
/// refresh with no approval for the specific command. It also predates `ro.debuggable` being
/// understood as the actual gate on the AOSP test entry point, and root does not open that gate —
/// the receiver is registered at class initialisation from a build property.
///
/// The probe is now read-only and its answer only describes the device. A non-zero uid is the
/// normal answer and it is not a failure.
fn adbd_uid(app: &tauri::AppHandle, serial: &str) -> Option<u32> {
    shell(app, serial, &["id", "-u"])
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok())
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

/// Local-simulator prerequisites, each an explicit five-state answer.
///
/// Reported separately from installation because they fail independently: a simulator can be
/// installed and still be unable to post a notification (Android 13+ POST_NOTIFICATIONS denied) or
/// unable to take over the screen (Android 14+ USE_FULL_SCREEN_INTENT denied). Both degrade the
/// result without making the send fail, so the desktop has to be able to say which happened
/// rather than reporting a flat success or a flat failure.
#[derive(Debug, Clone, Serialize, Default)]
pub struct SimulatorCapabilities {
    pub installed: bool,
    pub post_notifications: PlatformState,
    pub full_screen_intent: PlatformState,
    pub notifications_enabled: PlatformState,
    pub post_notifications_detail: String,
    pub full_screen_intent_detail: String,
}

fn simulator_capabilities(
    app: &tauri::AppHandle,
    serial: &str,
    evidence: &mut Vec<ProbeEvidence>,
) -> SimulatorCapabilities {
    let mut capabilities = SimulatorCapabilities {
        installed: local_simulator_installed(app, serial),
        ..Default::default()
    };

    if !capabilities.installed {
        capabilities.post_notifications = PlatformState::NotPresent;
        capabilities.full_screen_intent = PlatformState::NotPresent;
        capabilities.notifications_enabled = PlatformState::NotPresent;
        return capabilities;
    }

    // POST_NOTIFICATIONS is only a runtime permission from Android 13 (API 33). Below that it does
    // not exist, and reporting a denial there would be a false alarm.
    let (sdk_text, sdk_evidence) = shell_captured(app, serial, &["getprop", "ro.build.version.sdk"]);
    let sdk = sdk_text.trim().parse::<u32>().ok();
    evidence.push(sdk_evidence);

    if sdk.is_some_and(|sdk| sdk >= 33) {
        let (dump, record) =
            shell_captured(app, serial, &["dumpsys", "package", LOCAL_SIMULATOR_PACKAGE]);
        capabilities.post_notifications = platform::parse_post_notifications(&dump);
        capabilities.post_notifications_detail =
            capabilities.post_notifications.label().to_string();
        evidence.push(ProbeEvidence {
            parsed: capabilities.post_notifications.label().to_string(),
            ..record
        });
    } else {
        capabilities.post_notifications = PlatformState::NotPresent;
        capabilities.post_notifications_detail = format!(
            "POST_NOTIFICATIONS does not exist below API 33 (device reports SDK {})",
            sdk_text.trim()
        );
    }

    // Android 14+ defaults USE_FULL_SCREEN_INTENT to denied for apps that are not calling or alarm
    // apps, which degrades a full-screen alert into a heads-up notification without any error.
    let (appop, record) = shell_captured(
        app,
        serial,
        &["appops", "get", LOCAL_SIMULATOR_PACKAGE, "USE_FULL_SCREEN_INTENT"],
    );
    capabilities.full_screen_intent = platform::parse_full_screen_intent(&appop);
    capabilities.full_screen_intent_detail = capabilities.full_screen_intent.label().to_string();
    evidence.push(ProbeEvidence {
        parsed: capabilities.full_screen_intent.label().to_string(),
        ..record
    });

    let (notifications, record) = shell_captured(app, serial, &["dumpsys", "notification"]);
    capabilities.notifications_enabled = if notifications.trim().is_empty() {
        PlatformState::Unknown
    } else if notifications.contains(&format!("{LOCAL_SIMULATOR_PACKAGE}: banned")) {
        PlatformState::Denied
    } else {
        // The package not being mentioned is the normal state for an app that has not posted
        // anything yet. That is not a ban, so it must not be reported as a denial.
        PlatformState::Unknown
    };
    evidence.push(ProbeEvidence {
        parsed: capabilities.notifications_enabled.label().to_string(),
        ..record
    });

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
                test_entrypoint_available: PlatformState::Unknown,
                test_entrypoint_reason: "No probe ran: the device is not yet authorised for debugging."
                    .to_string(),
                capability_stage: CapabilityStage::None,
                alert_mode: platform::AlertMode::Unavailable,
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
                test_entrypoint_available: PlatformState::Unknown,
                test_entrypoint_reason: "No probe ran: ADB reports the device as offline."
                    .to_string(),
                capability_stage: CapabilityStage::None,
                alert_mode: platform::AlertMode::Unavailable,
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
                test_entrypoint_available: PlatformState::Unknown,
                test_entrypoint_reason: "ADB transport is not in the device state; no probe ran."
                    .to_string(),
                capability_stage: CapabilityStage::None,
                alert_mode: platform::AlertMode::Unavailable,
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

        // Read-only identity probe. `Some(0)` means adbd already runs as root; it is never asked
        // to become root here.
        let root = adbd_uid(app, &serial) == Some(0);
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
        let cellbroadcast_package_was_found = cellbroadcast_package.is_some();

        // Whether the AOSP test entry point exists is a build property, not a permission. Stating
        // it here means the operator learns it before selecting an action, rather than from a
        // broadcast that reported success and produced nothing.
        let test_entrypoint =
            platform::assess_test_entrypoint(&debuggable, cellbroadcast_package.as_deref());
        let capabilities = if local_simulator {
            Some(simulator_capabilities(app, &serial, &mut Vec::new()))
        } else {
            None
        };

        let (state, support_level) = if local_simulator {
            notes.push(
                "Root-free local simulator is installed. Send uses our explicit test receiver and \
                 the notification/full-screen pipeline. This is a local app notification, not a \
                 Cell Broadcast."
                    .to_string(),
            );
            match &capabilities {
                Some(capabilities) if capabilities.post_notifications == PlatformState::Denied => {
                    notes.push(
                        "Notifications are denied for the local simulator, so Android will run the \
                         receiver and drop the alert silently. Grant notifications for that app."
                            .to_string(),
                    );
                }
                Some(capabilities) if capabilities.post_notifications == PlatformState::Unknown => {
                    notes.push(
                        "Notification permission could not be read. The simulator may still work; \
                         the state is unknown rather than denied."
                            .to_string(),
                    );
                }
                _ => {}
            }
            if matches!(
                capabilities.as_ref().map(|c| c.full_screen_intent),
                Some(PlatformState::Denied)
            ) {
                notes.push(
                    "USE_FULL_SCREEN_INTENT is denied, so the alert will be a heads-up notification \
                     rather than a full-screen takeover."
                        .to_string(),
                );
            }
            (DeviceState::SimulatorReady, SupportLevel::LocalSimulator)
        } else if test_entrypoint.available == PlatformState::Granted {
            // The controlled path is gated on the build permitting the AOSP test receiver, which is
            // the thing that actually decides whether the broadcast can be delivered. Root is not
            // part of this condition: a rooted `user` build still has no test receiver, and a
            // `userdebug` build does not need adbd to run as root for an exported receiver.
            notes.push(format!(
                "Controlled target. AOSP test entry point: {}.",
                test_entrypoint.available.label()
            ));
            notes.push(test_entrypoint.reason.clone());
            (DeviceState::Ready, SupportLevel::Supported)
        } else if cellbroadcast_package.is_none() {
            notes.push(
                "No CellBroadcast receiver package was detected. Install the local simulator to \
                 test the alert UI on a stock device."
                    .to_string(),
            );
            (DeviceState::Unsupported, SupportLevel::Unsupported)
        } else {
            notes.push(test_entrypoint.reason.clone());
            notes.push(
                "Stock/non-root device. The local simulator provides a root-free alert UI path; it \
                 produces a local app notification, not a Cell Broadcast."
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
            test_entrypoint_available: test_entrypoint.available,
            test_entrypoint_reason: test_entrypoint.reason,
            capability_stage: platform::capability_stage(
                cellbroadcast_package_was_found,
                cellbroadcast_package_was_found,
                test_entrypoint.available,
            ),
            // `verified` is false here by construction: verification is a per-send logcat
            // observation, and this is a device profile built before any send happened.
            alert_mode: platform::alert_mode(
                test_entrypoint.available,
                local_simulator,
                false,
            ),
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

/// The alert channels this tool can address, with each channel's receive-side gate.
///
/// Exposed so the interface offers exactly the channels the encoding path will accept, rather than
/// re-implementing the catalogue in JavaScript. A second copy of this table is a second chance for
/// the two to disagree, and a UI listing a channel the backend then refuses is the same class of
/// defect as a broadcast action that does not exist.
#[tauri::command]
fn list_alert_channels() -> Vec<platform::AlertChannel> {
    platform::ALERT_CHANNELS.to_vec()
}

/// The result of a platform test-injection attempt.
#[derive(Debug, Clone, Serialize)]
pub struct PlatformSendResult {
    pub device_serial: String,
    pub body: String,
    pub pdu_hex: String,
    pub message_id: String,
    /// The 3GPP message identifier this attempt addressed.
    pub channel_id: u16,
    /// Its label from the [`platform::ALERT_CHANNELS`] catalogue.
    pub channel_label: String,
    /// The AOSP receive-side preference that decides whether the alert is raised.
    pub channel_gate: String,
    /// What the operator must change for this channel, if anything.
    pub channel_requirement: String,
    /// Whether the channel is on for a device that has never been configured.
    pub channel_enabled_by_default: bool,
    pub entrypoint_available: PlatformState,
    pub stage: CapabilityStage,
    /// The rung of [`platform::VerificationStage`] this attempt actually reached.
    ///
    /// Distinct from `stage`: `stage` describes the strongest general capability observed, and this
    /// describes how far *this* attempt got down the verification ladder. It is what the UI reports
    /// as "reached / did not reach", so a send that exited 0 but left no downstream log lands on
    /// `TRIGGER_SENT` and not on anything that reads as delivery.
    pub verification_stage: platform::VerificationStage,
    pub state: String,
    pub message: String,
    pub failure: Option<String>,
    pub evidence: Vec<String>,
    pub logcat_excerpt: String,
    /// The platform gate that discarded the message, when the platform said so, with the verbatim
    /// device line. Present only when the run reached the Cell Broadcast stack and was suppressed.
    pub suppression: Option<platform::Suppression>,
    /// Every command that was run, with its raw output. Never collapsed.
    pub diagnostics: Vec<ProbeEvidence>,
}

/// Probe a device's Cell Broadcast capability without changing anything.
///
/// Read-only, and it returns the raw output of every command behind its conclusions so the
/// operator can check the parsing rather than trust it.
#[tauri::command]
fn platform_diagnostics(app: tauri::AppHandle, serial: String) -> Result<platform::PlatformProbe, String> {
    let mut evidence = Vec::new();

    let (debuggable, record) = shell_captured(&app, &serial, &["getprop", platform::DEBUGGABLE_PROPERTY]);
    let debuggable = debuggable.trim().to_string();
    let mut entrypoint_record = record;
    entrypoint_record.parsed = format!("ro.debuggable={debuggable:?}");
    evidence.push(entrypoint_record);

    let (build_type, record) = shell_captured(&app, &serial, &["getprop", "ro.build.type"]);
    evidence.push(record);

    let candidates = cellbroadcast_candidates(&app, &serial);
    let cellbroadcast_package = candidates.first().cloned();

    // Package presence must be paired with an actual receiver declaration. A package name is a
    // string; it is not evidence that the component exists.
    let receiver_declared = match &cellbroadcast_package {
        Some(package) => {
            let (dump, mut record) =
                shell_captured(&app, &serial, &["dumpsys", "package", package]);
            let declared = dump.contains("CellBroadcastReceiver")
                || dump.contains("CellBroadcastAlertService")
                || dump.contains("cellbroadcastreceiver");
            record.parsed = if declared {
                "CellBroadcast receiver/service declared in the package manifest".to_string()
            } else if dump.trim().is_empty() {
                "no package dump returned".to_string()
            } else {
                "package present but no CellBroadcast component found in its dump".to_string()
            };
            evidence.push(record);
            if declared {
                PlatformState::Granted
            } else if dump.trim().is_empty() {
                PlatformState::Unknown
            } else {
                PlatformState::NotPresent
            }
        }
        None => PlatformState::NotPresent,
    };

    let test_entrypoint =
        platform::assess_test_entrypoint(&debuggable, cellbroadcast_package.as_deref());

    let local_simulator_installed = local_simulator_installed(&app, &serial);
    let capabilities = simulator_capabilities(&app, &serial, &mut evidence);

    // Capability is the *lowest* honest stage: what the device is, not what we hope it is.
    let stage = if receiver_declared == PlatformState::Granted
        && test_entrypoint.available == PlatformState::Granted
    {
        CapabilityStage::TestEntryPointDiscovered
    } else if receiver_declared == PlatformState::Granted {
        CapabilityStage::ReceiverDiscovered
    } else if cellbroadcast_package.is_some() {
        CapabilityStage::PackagePresent
    } else {
        CapabilityStage::None
    };

    let summary = if test_entrypoint.available == PlatformState::Granted {
        format!(
            "{} The platform Cell Broadcast test path is available on this build.",
            test_entrypoint.reason
        )
    } else if test_entrypoint.available == PlatformState::Denied {
        format!(
            "{} Use the local simulator command instead: it posts a local app alert and needs no \
             root, but it is not a Cell Broadcast.",
            test_entrypoint.reason
        )
    } else {
        test_entrypoint.reason.clone()
    };

    Ok(platform::PlatformProbe {
        build_type: (!build_type.trim().is_empty()).then(|| build_type.trim().to_string()),
        debuggable: (!debuggable.is_empty()).then_some(debuggable),
        test_entrypoint,
        cellbroadcast_candidates: candidates,
        cellbroadcast_package,
        receiver_declared,
        local_simulator_installed,
        post_notifications: capabilities.post_notifications,
        full_screen_intent: capabilities.full_screen_intent,
        notifications_enabled: capabilities.notifications_enabled,
        stage,
        summary,
        evidence,
    })
}

/// The diagnostic report, plus the same report rendered as readable Markdown.
///
/// Both forms are returned from one call so the controller never has to re-run the commands to
/// produce the written form. The Markdown is what the operator attaches to an issue; the structured
/// form is what the UI renders.
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticBundle {
    pub report: diagnostics::DiagnosticReport,
    pub markdown: String,
}

/// Collect a structured, read-only diagnostic report for one device.
///
/// Read-only by construction: every command below inspects state. Nothing here writes a setting,
/// installs, clears data or touches a user file, so the operator can run it on their own phone
/// without approval for a specific command.
///
/// The report is assembled from raw command output by pure functions in `diagnostics`, so the same
/// text can be re-parsed in a test. Where a command fails or returns nothing, the corresponding
/// field stays `Unknown` rather than becoming a negative answer — an unreadable dump is not a
/// denial, and a package name is not a component declaration.
#[tauri::command]
fn device_diagnostics(
    app: tauri::AppHandle,
    serial: String,
) -> Result<DiagnosticBundle, String> {
    let mut commands: Vec<diagnostics::CommandResult> = Vec::new();

    let capture = |label: &str, args: &[&str]| {
        let (stdout, record) = shell_captured(&app, &serial, args);
        (stdout, record, label.to_string())
    };

    // 1. Every property the report needs, in one `getprop` invocation.
    let (properties_raw, record, label) = capture("device properties", &["getprop"]);
    let mut record = record;
    let properties = diagnostics::parse_getprop_batch(&properties_raw);
    record.parsed = format!("{} properties parsed", properties.len());
    commands.push(diagnostics::CommandResult {
        label,
        command: record.command,
        exit_code: record.exit_code,
        stdout: record.stdout,
        stderr: record.stderr,
        parsed: record.parsed,
    });

    // 2. Package inventory with APK paths, so an APEX-shipped module is visible as such.
    let (packages_raw, record, label) = capture("package inventory", &["pm", "list", "packages", "-f"]);
    let listed = diagnostics::parse_pm_list_packages_f(&packages_raw);
    let mut record = record;
    record.parsed = format!("{} installed packages listed", listed.len());
    commands.push(diagnostics::CommandResult {
        label,
        command: record.command,
        exit_code: record.exit_code,
        stdout: record.stdout,
        stderr: record.stderr,
        parsed: record.parsed,
    });

    // 3. For each package that could plausibly participate, read its dump once.
    let mut records = Vec::new();
    for (name, path) in &listed {
        let Some((relevance, _)) = diagnostics::classify_package(name) else {
            continue;
        };

        let (dump, record) = shell_captured(&app, &serial, &["dumpsys", "package", name]);
        let dump_readable = !dump.trim().is_empty();
        let receivers = if dump_readable {
            diagnostics::parse_receivers(&dump)
                .into_iter()
                .filter(diagnostics::receiver_is_cellbroadcast)
                .count()
        } else {
            0
        };
        let note = if !dump_readable {
            "dump returned nothing; capabilities from this package are UNKNOWN, not denied"
        } else if receivers > 0 {
            "Cell Broadcast receiver declaration found"
        } else {
            "no Cell Broadcast receiver declaration in this package"
        };

        commands.push(diagnostics::CommandResult {
            label: format!("package dump: {} [{}]", name, relevance.label()),
            command: record.command,
            exit_code: record.exit_code,
            stdout: record.stdout,
            stderr: record.stderr,
            parsed: note.to_string(),
        });

        if let Some(entry) = diagnostics::package_record(name, vec![path.clone()], &dump) {
            records.push(entry);
        }
    }

    // Most relevant first, so a reader sees the component before the carrier configuration.
    records.sort_by(|a, b| {
        a.relevance
            .cmp(&b.relevance)
            .then_with(|| a.name.cmp(&b.name))
    });

    let report = diagnostics::build_report(&properties, &records, commands);
    let markdown = diagnostics::render_report(&report);
    Ok(DiagnosticBundle { report, markdown })
}

/// Inject one Cell Broadcast through the AOSP telephony test entry point.
/// This is the root-free path: `GsmInboundSmsHandler` registers a receiver for
/// `com.android.internal.telephony.gsm.TEST_TRIGGER_CELL_BROADCAST` on `eng`/`userdebug` builds,
/// and it decodes a Cell Broadcast PDU placed in the `pdu_string` extra. Running it needs only the
/// ADB shell identity, not root, because the receiver is not a `<protected-broadcast>`.
///
/// Four things this deliberately does not do:
///
/// * It does not run on a production build. `ro.debuggable=0` means the receiver was never
///   registered, so the broadcast would be accepted by `am` and silently discarded. Returning an
///   honest "not available" is worth more than a command that looks like it worked.
/// * It does not build a shell string. Arguments go to `Command::new(adb).args(..)` as a vector, so
///   nothing in the body, the package name or the PDU can be reinterpreted by a shell on either
///   side of the transport.
/// * It does not claim delivery from the exit code. The verdict comes from downstream logcat
///   markers, and an accepted-but-silent broadcast is reported as such.
/// * It does not silently pick a channel. `channel` selects the message identifier, and the
///   receive-side gate for the chosen channel is reported alongside the result, because the ETWS
///   test channel this tool used to hardcode is disabled by default while ETWS primary is not.
#[tauri::command]
fn send_platform_test_alert(
    app: tauri::AppHandle,
    serial: String,
    body: String,
    channel: Option<u16>,
) -> Result<PlatformSendResult, String> {
    let body = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if !body.starts_with(REQUIRED_PREFIX) {
        return Err(format!(
            "Refusing to send: the body must begin with {REQUIRED_PREFIX} so the alert is \
             unambiguously identifiable as a test."
        ));
    }

    let channel_id = match channel {
        Some(id) => id,
        None => platform::default_alert_channel().message_id,
    };
    let channel = match platform::alert_channel(channel_id) {
        Some(c) => c,
        None => {
            let known: Vec<String> = platform::ALERT_CHANNELS
                .iter()
                .map(|c| format!("0x{:04X} ({})", c.message_id, c.label))
                .collect();
            return Err(format!(
                "Unknown alert channel 0x{channel_id:04X}. Known channels: {}",
                known.join(", ")
            ));
        }
    };

    let mut diagnostics = Vec::new();
    let mut result = PlatformSendResult {
        device_serial: serial.clone(),
        body: body.clone(),
        pdu_hex: String::new(),
        message_id: String::new(),
        channel_id: channel.message_id,
        channel_label: channel.label.to_string(),
        channel_gate: channel.gate.to_string(),
        channel_requirement: channel.requirement.to_string(),
        channel_enabled_by_default: channel.enabled_by_default,
        entrypoint_available: PlatformState::Unknown,
        stage: CapabilityStage::None,
        verification_stage: platform::VerificationStage::NativePathSelected,
        state: "UNKNOWN".to_string(),
        message: String::new(),
        failure: None,
        evidence: Vec::new(),
        logcat_excerpt: String::new(),
        suppression: None,
        diagnostics: Vec::new(),
    };

    // 1. Is the test entry point present at all?
    let (debuggable, record) = shell_captured(&app, &serial, &["getprop", platform::DEBUGGABLE_PROPERTY]);
    diagnostics.push(record);
    let debuggable = debuggable.trim().to_string();

    let candidates = cellbroadcast_candidates(&app, &serial);
    let receiver = candidates.first().cloned();
    let entrypoint = platform::assess_test_entrypoint(&debuggable, receiver.as_deref());
    result.entrypoint_available = entrypoint.available;

    if entrypoint.available != PlatformState::Granted {
        result.state = "BLOCKED".to_string();
        result.message = entrypoint.reason.clone();
        result.failure = Some(entrypoint.available.label().to_string());
        // The device and its capabilities were read, but the native path could not be selected for
        // this build. That is the rung the attempt stopped on, and it is not a delivery claim.
        result.verification_stage = platform::VerificationStage::CapabilitiesDetected;
        result.diagnostics = diagnostics;
        return Ok(result);
    }

    // 2. Build the PDU here, from validated input, so the device receives a checked message.
    let serial_number = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u16)
        .unwrap_or(0))
        .wrapping_add(1);
    let pdu = platform::cb_pdu(channel.message_id, &body, serial_number)?;
    let pdu_hex: String = pdu.iter().map(|byte| format!("{byte:02X}")).collect();
    if let Some(header) = platform::parse_cb_header(&pdu) {
        result.message_id = format!("0x{:04X} {}", header.message_id, header.message_id_label());
    }
    result.pdu_hex = pdu_hex.clone();
    result.evidence.push(format!(
        "PDU {} bytes, {pdu_hex}",
        pdu.len()
    ));
    result.evidence.push(format!(
        "channel {}: receive-side gate is {}; {}",
        channel.label,
        channel.gate,
        if channel.enabled_by_default {
            "enabled on a default device".to_string()
        } else {
            format!("disabled by default — {}", channel.requirement)
        }
    ));

    // 3. Clear logcat so the evidence belongs only to this attempt.
    let (_cleared, record) = shell_captured(&app, &serial, &["logcat", "-c"]);
    diagnostics.push(record);

    // 4. Send. Every argument is a separate argv element: no shell string is ever constructed.
    let args = platform_test_alert_args(&serial, &pdu_hex);
    let (stdout, record) = run_captured(&app, &args);
    // `am` reports a missing receiver on stdout while still exiting 0 on some builds, so the
    // wording is checked as well as the exit code.
    let accepted = record.exit_code == Some(0) && !stdout.contains("Broadcast failed");
    result.evidence.push(format!(
        "am broadcast exit={:?} accepted_by_am={accepted}",
        record.exit_code
    ));
    diagnostics.push(record);

    // 5. Wait for the pipeline, then read what actually happened.
    thread::sleep(Duration::from_millis(2500));
    let (logcat, record) = shell_captured(&app, &serial, &["logcat", "-d", "-t", "800"]);
    diagnostics.push(record);

    let platform_evidence = platform::scan_platform_logcat(&logcat);
    result.stage = platform_evidence.stage();
    result.verification_stage = platform_evidence.verification_stage();
    result.logcat_excerpt = truncate_for_log(&logcat, 3000);

    // 6. The verdict comes from downstream evidence, never from the exit code.
    //
    // Suppression is checked *first*. A gated message can still leave positive traces: the
    // `CBAlertService: onStartCommand` line fires before the testing-mode and channel-range checks
    // run, so a capture can contain both "the service started" and "the platform dropped it". The
    // drop is the more specific and more final fact, and checking `pipeline_ran()` first would have
    // reported such a run as `ALERT_DISPLAYED`.
    if platform_evidence.was_suppressed() {
        // The platform processed the message and then deliberately dropped it, and said why. This
        // is a definite answer and a different one from "nothing was observed": the injection
        // reached the Cell Broadcast stack, so re-running it cannot help. Report the gate instead
        // of leaving the operator with an unexplained silence.
        let suppression = platform_evidence.suppression.as_ref().expect("was_suppressed()");
        result.state = "SUPPRESSED_BY_PLATFORM".to_string();
        result.message = format!(
            "The message reached the Cell Broadcast stack and the platform then discarded it, \
             because {}. The injection itself worked; the device's own configuration is what \
             prevented the alert.",
            suppression.explain()
        );
        result.failure = Some("SUPPRESSED_BY_PLATFORM".to_string());
        result.suppression = Some(suppression.clone());
    } else if platform_evidence.pipeline_ran() {
        result.state = "ALERT_DISPLAYED".to_string();
        result.message = format!(
            "Platform Cell Broadcast pipeline evidence found: reached stage {}.",
            result.stage.label()
        );
    } else if accepted {
        result.state = "ACCEPTED_NO_EVIDENCE".to_string();
        result.message = "`am broadcast` was accepted, but no downstream Cell Broadcast log line \
                          followed. Per this project's rule, that is not a delivery. The command \
                          was accepted; Android did not act on it."
            .to_string();
        result.failure = Some("NO_DOWNSTREAM_EVIDENCE".to_string());
    } else {
        result.state = "REJECTED".to_string();
        result.message = "The platform test broadcast was not accepted by this device.".to_string();
        result.failure = Some("BROADCAST_REJECTED".to_string());
    }

    result.diagnostics = diagnostics;
    if let Some(first) = result.evidence.first().cloned() {
        emit_log(&app, format!("Platform test alert: {first}"), "info");
    }
    Ok(result)
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
        let send_started = Instant::now();
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
            duration_ms: None,
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
            let mut probe_evidence = Vec::new();
            let capabilities = simulator_capabilities(&app, &serial, &mut probe_evidence);
            diag(
                &app,
                &mut result,
                stage::CAPABILITY_CHECK,
                "simulator capability probe",
                Some(format!(
                    "installed={} post_notifications={} ({}) full_screen_intent={} ({}) notifications_enabled={}",
                    capabilities.installed,
                    capabilities.post_notifications.label(),
                    capabilities.post_notifications_detail,
                    capabilities.full_screen_intent.label(),
                    capabilities.full_screen_intent_detail,
                    capabilities.notifications_enabled.label(),
                )),
                None,
                None,
                if capabilities.installed { "info" } else { "warn" },
            );

            for entry in &probe_evidence {
                diag(
                    &app,
                    &mut result,
                    stage::CAPABILITY_CHECK,
                    format!("probe: {}", entry.command),
                    Some(format!(
                        "exit={:?} parsed={}",
                        entry.exit_code,
                        if entry.parsed.is_empty() { "-" } else { &entry.parsed }
                    )),
                    (!entry.stdout.trim().is_empty()).then(|| truncate_for_log(&entry.stdout, 1200)),
                    (!entry.stderr.trim().is_empty()).then(|| truncate_for_log(&entry.stderr, 600)),
                    "info",
                );
            }

            // Only an explicit denial blocks the send. An UNKNOWN result means the probe could not
            // read the grant state, which is a reason to warn, not a reason to refuse: refusing on
            // unknown is how this tool previously told operators to grant a permission that was
            // already granted.
            if capabilities.post_notifications == PlatformState::Denied {
                let _ = set_tx(&app, &tx_store, &serial, None);
                fail_stage(
                    &app,
                    &mut result,
                    stage::CAPABILITY_CHECK,
                    "NOTIFICATION_PERMISSION_DENIED",
                    "POST_NOTIFICATIONS",
                    "Notification permission is explicitly denied for Emergency Simulator Local. \
                     Android will run the receiver and then drop the notification silently. Grant \
                     notifications for that app on the phone, then retry.",
                    None,
                    None,
                );
                return Ok(result);
            }

            if capabilities.post_notifications == PlatformState::Unknown {
                diag(
                    &app,
                    &mut result,
                    stage::CAPABILITY_CHECK,
                    "notification permission state is unknown",
                    Some(
                        "The probe could not read an explicit grant state. Continuing, but a \
                         denied notification permission would silently suppress the alert."
                            .to_string(),
                    ),
                    None,
                    None,
                    "warn",
                );
            }

            if capabilities.full_screen_intent == PlatformState::Denied {
                diag(
                    &app,
                    &mut result,
                    stage::CAPABILITY_CHECK,
                    "full-screen intent is denied",
                    Some(
                        "USE_FULL_SCREEN_INTENT is denied, so Android will show a heads-up \
                         notification instead of a full-screen alert. The alert still posts; only \
                         the presentation differs."
                            .to_string(),
                    ),
                    None,
                    None,
                    "warn",
                );
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

            let broadcast_started = Instant::now();
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

            let broadcast_ms = broadcast_started.elapsed().as_millis() as u64;

            diag_timed(
                &app,
                &mut result,
                stage::ADB_BROADCAST_RESULT,
                "adb shell am broadcast",
                Some(format!("exit={:?}", output.status.code())),
                Some(stdout.clone()).filter(|s| !s.is_empty()),
                Some(stderr.clone()).filter(|s| !s.is_empty()),
                broadcast_ms,
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
            result.duration_ms = Some(send_started.elapsed().as_millis() as u64);
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

    /// BUG-021: the injection command named a bare package with `-n`, included a `format` extra the
    /// handler does not read, and omitted the `phone_id` selector. `-n` takes `package/class`, so
    /// `am` rejected the arguments outright and the attempt could never have reached the receiver on
    /// any device. Asserted as a whole argv: the point is that a stray `-n` or a wrong extra must
    /// fail the build rather than be read as a device limitation.
    ///
    /// `phone_id` is now deliberately *absent* rather than pinned to `0`. See the note on
    /// `platform_test_alert_args`: pinning it makes the receiver return silently on a device whose
    /// active subscription is not phone 0, which is indistinguishable from a missing receiver.
    #[test]
    fn platform_test_injection_matches_the_aosp_contract() {
        let args = platform_test_alert_args("R5CXA1B2C3D", "00001100");
        assert_eq!(
            args,
            vec![
                "-s",
                "R5CXA1B2C3D",
                "shell",
                "am",
                "broadcast",
                "-a",
                "com.android.internal.telephony.gsm.TEST_TRIGGER_CELL_BROADCAST",
                "--es",
                "pdu_string",
                "00001100",
            ]
        );
        // The regressions, stated directly so the reason survives a refactor of the vector.
        assert!(!args.contains(&"-n"), "the test receiver has no manifest component to target");
        assert!(!args.windows(2).any(|w| w == ["--es", "format"]));
        assert!(
            !args.contains(&"phone_id"),
            "pinning phone_id makes the receiver return silently off phone 0"
        );
        assert!(
            !args.windows(2).any(|w| w == ["--ei", "phone_id"]),
            "phone_id must not be pinned to a fixed slot"
        );
    }

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
            platform_diagnostics,
            device_diagnostics,
            send_platform_test_alert,
            list_alert_channels,
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
