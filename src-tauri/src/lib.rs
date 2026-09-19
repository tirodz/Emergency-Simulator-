use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    process::{Command, Output},
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeviceState {
    Ready,
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
    RootRequired,
    Untested,
    Unsupported,
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
    pub state: DeviceState,
    pub support_level: SupportLevel,
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
            "adb.exe",
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

fn find_cellbroadcast(app: &tauri::AppHandle, serial: &str) -> Option<String> {
    let text = shell(app, serial, &["pm", "list", "packages"]).ok()?;

    text.lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix("package:"))
        .find(|package| package.to_ascii_lowercase().contains("cellbroadcast"))
        .map(str::to_string)
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

fn parse_devices(app: &tauri::AppHandle) -> Result<Vec<Device>, String> {
    let text = adb_call(app, &["devices", "-l"])?;
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
                state: DeviceState::Unauthorized,
                support_level: SupportLevel::Untested,
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
                state: DeviceState::Offline,
                support_level: SupportLevel::Untested,
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
                state: DeviceState::Unknown,
                support_level: SupportLevel::Untested,
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
        let cellbroadcast_package = find_cellbroadcast(app, &serial);

        let samsung_a35 = model.to_ascii_lowercase().contains("sm-a356")
            || model.to_ascii_lowercase().contains("galaxy a35");

        let mut notes = Vec::new();

        if samsung_a35 {
            notes.push(
                "Galaxy A35 detected. Stock firmware is handled with read-only ADB diagnostics."
                    .to_string(),
            );
        }

        let (state, support_level) = if cellbroadcast_package.is_none() {
            notes.push("No CellBroadcast receiver package was detected.".to_string());
            (DeviceState::Unsupported, SupportLevel::Unsupported)
        } else if !root {
            notes.push(
                "Stock/non-root device. The protected CellBroadcast injection path is not demonstrated on this production build."
                    .to_string(),
            );
            (DeviceState::NoRoot, SupportLevel::RootRequired)
        } else {
            notes.push(
                "Rooted/userdebug controlled target. The genuine Android CellBroadcast test path is available."
                    .to_string(),
            );
            (DeviceState::Ready, SupportLevel::Supported)
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
            state,
            support_level,
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

    let command = format!(
        "cat /data/local/tmp/emergency-sim-prefs.xml > '{remote}'; rm -f /data/local/tmp/emergency-sim-prefs.xml"
    );

    shell(app, serial, &["sh", "-c", &command]).map(|_| ())
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
            "alertinject.jar",
            "android/alertinject/out/alertinject.jar",
            "alertinject/alertinject.jar",
        ],
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

        thread::sleep(Duration::from_secs(2));

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
        version: "2.0.0".to_string(),
        adb_source: source,
        injector: injector_path(&app).is_some(),
    })
}

#[tauri::command]
fn list_devices(app: tauri::AppHandle) -> Result<Vec<Device>, String> {
    parse_devices(&app)
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
        };

        if !normalized_body.starts_with(REQUIRED_PREFIX) {
            result.failure = Some("INVALID_BODY".to_string());
            result.message = "The message must begin with TEST.".to_string();
            return Ok(result);
        }

        if normalized_body.len() > 300 {
            result.failure = Some("INVALID_BODY".to_string());
            result.message = "The message is longer than 300 characters.".to_string();
            return Ok(result);
        }

        if let Some(reason) = gate_for(&tx_store, &serial) {
            result.failure = Some("DUPLICATE_SEND_BLOCKED".to_string());
            result.message = reason;
            return Ok(result);
        }

        emit_log(&app, format!("Inspecting target {serial}"), "info");

        let device = parse_devices(&app)?
            .into_iter()
            .find(|device| device.serial == serial)
            .ok_or_else(|| "The selected device is no longer attached.".to_string())?;

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
            };

            emit_log(
                &app,
                format!("Dry run complete for {serial}; no device changes"),
                "ok",
            );
            return Ok(result);
        }

        if !matches!(device.state, DeviceState::Ready) {
            result.failure = Some(
                match device.state {
                    DeviceState::Unauthorized => "DEVICE_UNAUTHORIZED",
                    DeviceState::Offline => "DEVICE_OFFLINE",
                    DeviceState::NoRoot => "NO_ROOT",
                    DeviceState::Unsupported => "CELLBROADCAST_MISSING",
                    DeviceState::Unknown => "DEVICE_UNKNOWN",
                    DeviceState::Ready => "UNKNOWN",
                }
                .to_string(),
            );

            result.message = match device.state {
                DeviceState::Unauthorized => {
                    "Accept the USB debugging authorization prompt on the phone first."
                        .to_string()
                }
                DeviceState::Offline => {
                    "ADB reports this device as offline.".to_string()
                }
                DeviceState::NoRoot => {
                    "This stock/non-root device cannot use the controlled protected test path."
                        .to_string()
                }
                DeviceState::Unsupported => {
                    "No CellBroadcast receiver package was detected.".to_string()
                }
                DeviceState::Unknown => {
                    "The device is not in a known ADB-ready state.".to_string()
                }
                DeviceState::Ready => "Device is ready.".to_string(),
            };

            return Ok(result);
        }

        let package = device
            .cellbroadcast_package
            .clone()
            .ok_or_else(|| "No CellBroadcast receiver package was detected.".to_string())?;

        emit_log(
            &app,
            format!("CellBroadcast receiver found: {package}"),
            "ok",
        );

        let test_mode = prepare_test_mode(
            &app,
            &serial,
            &package,
            false,
            &|message| emit_log(&app, message, "info"),
        )?;

        if !test_mode {
            result.failure = Some("TEST_MODE_DISABLED".to_string());
            result.message =
                "The controlled test-alert preferences could not be established."
                    .to_string();
            return Ok(result);
        }

        if cancel_requested(&cancel_store, &serial) {
            clear_cancel(&cancel_store, &serial);
            result.state = "CANCELLED".to_string();
            result.failure = Some("USER_CANCELLED".to_string());
            result.message = "Operation cancelled before delivery.".to_string();
            return Ok(result);
        }

        set_tx(&app, &tx_store, &serial, Some(TxState::Busy))?;

        let _ = adb_call(&app, &["-s", &serial, "logcat", "-c"]);

        if let Err(error) = push_injector(&app, &serial) {
            let _ = set_tx(&app, &tx_store, &serial, None);
            result.failure = Some("INJECTOR_FAILURE".to_string());
            result.message = error.clone();
            emit_log(&app, error, "error");
            return Ok(result);
        }

        emit_log(
            &app,
            "Injector pushed to the controlled device",
            "ok",
        );

        emit_log(
            &app,
            format!("Sending ETWS TEST; category {SERVICE_CATEGORY}"),
            "info",
        );

        let category = SERVICE_CATEGORY.to_string();

        let output = command_output(
            &app,
            &[
                "-s",
                &serial,
                "shell",
                "CLASSPATH=/data/local/tmp/alertinject.jar",
                "app_process",
                "/system/bin",
                INJECTOR_CLASS,
                &category,
                &normalized_body,
            ],
        )?;

        result.injector_exit_code = output.status.code();

        let injector_text = output_text(&output);
        if let Some(first_line) = injector_text.lines().next() {
            emit_log(
                &app,
                format!("Injector: {first_line}"),
                "info",
            );
        }

        collect_evidence(
            &app,
            &serial,
            &cancel_store,
            &mut result,
            &tx_store,
        )?;

        if result.state == "FAILED"
            && result.failure.as_deref() == Some("BROADCAST_REJECTED")
        {
            let _ = set_tx(&app, &tx_store, &serial, None);
        }

        let _ = persist_transactions(&app, &tx_store);
        clear_cancel(&cancel_store, &serial);

        let kind = if result.state == "ALERT_DISPLAYED" {
            "ok"
        } else {
            "warn"
        };

        emit_log(
            &app,
            format!("Result: {}", result.state),
            kind,
        );

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
            list_devices,
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
