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
use tauri::{Emitter, State};

const SERVICE_CATEGORY: u32 = 4355;
const REQUIRED_PREFIX: &str = "TEST";
const DEFAULT_BODY: &str = "TEST ALERT - SIMULATION";
const INJECTOR_CLASS: &str = "org.emergencysim.alertinject.AlertInjector";
const INJECTOR_REMOTE: &str = "/data/local/tmp/alertinject.jar";
const EVIDENCE_TIMEOUT_SECS: u64 = 45;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeviceState { Ready, Unauthorized, Offline, NoRoot, Unsupported, Unknown }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SupportLevel { Supported, RootRequired, Untested, Unsupported }

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
pub struct AppInfo { pub version: String, pub adb_source: String, pub injector: bool }

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
struct ActivityEvent { message: String, kind: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum TxState { Busy, Delivered, Uncertain }

#[derive(Debug, Default)]
struct TxStore { map: Mutex<HashMap<String, TxState>>, error: Mutex<Option<String>> }

#[derive(Debug, Default)]
struct CancelStore { set: Mutex<HashSet<String>> }

type SharedTx = Arc<TxStore>;
type SharedCancel = Arc<CancelStore>;

fn emit_log(app: &tauri::AppHandle, message: impl Into<String>, kind: &str) {
    let _ = app.emit("activity", ActivityEvent { message: message.into(), kind: kind.to_string() });
}

fn data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| format!("could not resolve application data directory: {e}"))
}

fn tx_path(app: &tauri::AppHandle) -> Result<PathBuf, String> { Ok(data_dir(app)?.join("transactions.json")) }

fn load_transactions(app: &tauri::AppHandle, store: &TxStore) {
    let path = match tx_path(app) {
        Ok(p) => p,
        Err(e) => { *store.error.lock().unwrap() = Some(e); return; }
    };
    if !path.exists() { return; }
    match fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<HashMap<String, TxState>>(&s).ok()) {
        Some(mut map) => {
            for value in map.values_mut() {
                if matches!(value, TxState::Busy) { *value = TxState::Uncertain; }
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
    if let Some(err) = store.error.lock().unwrap().clone() { return Err(err); }
    let path = tx_path(app)?;
    let parent = path.parent().ok_or("transaction path has no parent")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let encoded = serde_json::to_vec_pretty(&*store.map.lock().unwrap()).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, encoded).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    if path.exists() { fs::remove_file(&path).map_err(|e| e.to_string())?; }
    fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(())
}

fn set_tx(app: &tauri::AppHandle, store: &TxStore, serial: &str, state: Option<TxState>) -> Result<(), String> {
    if let Some(err) = store.error.lock().unwrap().clone() { return Err(err); }
    {
        let mut map = store.map.lock().unwrap();
        match state { Some(value) => { map.insert(serial.to_string(), value); }, None => { map.remove(serial); } }
    }
    persist_transactions(app, store)
}

fn gate_for(store: &TxStore, serial: &str) -> Option<String> {
    if let Some(err) = store.error.lock().unwrap().clone() { return Some(err); }
    match store.map.lock().unwrap().get(serial) {
        None => None,
        Some(TxState::Busy) => Some("A test alert is already being processed for this device.".to_string()),
        Some(TxState::Delivered) => Some("The previous Android test alert is still outstanding. Dismiss it on the phone, then acknowledge it here before sending again.".to_string()),
        Some(TxState::Uncertain) => Some("The previous attempt had an uncertain outcome. Check the phone screen before sending again, then acknowledge the device state.".to_string()),
    }
}

fn cancel_requested(store: &CancelStore, serial: &str) -> bool { store.set.lock().unwrap().contains(serial) }
fn clear_cancel(store: &CancelStore, serial: &str) { store.set.lock().unwrap().remove(serial); }
fn set_cancel(store: &CancelStore, serial: &str) { store.set.lock().unwrap().insert(serial.to_string()); }

fn resource_candidate(app: &tauri::AppHandle, names: &[&str]) -> Option<PathBuf> {
    if let Ok(dir) = app.path().resource_dir() {
        for name in names { let p = dir.join(name); if p.exists() { return Some(p); } }
    }
    let cwd = std::env::current_dir().ok()?;
    for name in names { let p = cwd.join(name); if p.exists() { return Some(p); } }
    None
}

fn adb_path(app: &tauri::AppHandle) -> (PathBuf, String) {
    if let Some(p) = resource_candidate(app, &["adb.exe", "platform-tools/adb.exe", "packaging/platform-tools/adb.exe"]) {
        return (p, "BUNDLED".to_string());
    }
    if let Ok(path) = std::env::var("EMERGENCY_SIMULATOR_ADB") {
        let p = PathBuf::from(path);
        if p.exists() { return (p, "ENVIRONMENT".to_string()); }
    }
    (PathBuf::from("adb"), "PATH".to_string())
}

fn command_output(app: &tauri::AppHandle, args: &[&str]) -> Result<Output, String> {
    let (adb, _) = adb_path(app);
    let mut cmd = Command::new(adb);
    cmd.args(args);
    #[cfg(windows)] {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd.output().map_err(|e| format!("ADB could not be started: {e}"))
}

fn output_text(out: &Output) -> String {
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    let err = String::from_utf8_lossy(&out.stderr);
    if !err.trim().is_empty() { text.push('\n'); text.push_str(err.trim()); }
    text
}

fn adb_call(app: &tauri::AppHandle, args: &[&str]) -> Result<String, String> {
    let out = command_output(app, args)?;
    let text = output_text(&out);
    if !out.status.success() { return Err(text.trim().to_string()); }
    Ok(text)
}

fn shell(app: &tauri::AppHandle, serial: &str, parts: &[&str]) -> Result<String, String> {
    let mut args: Vec<&str> = vec!["-s", serial, "shell"];
    args.extend_from_slice(parts);
    adb_call(app, &args)
}

fn getprop(app: &tauri::AppHandle, serial: &str, key: &str) -> String {
    shell(app, serial, &["getprop", key]).unwrap_or_default().trim().to_string()
}

fn find_cellbroadcast(app: &tauri::AppHandle, serial: &str) -> Option<String> {
    let text = shell(app, serial, &["pm", "list", "packages"]).ok()?;
    text.lines().map(str::trim).filter_map(|line| line.strip_prefix("package:"))
        .find(|pkg| pkg.to_ascii_lowercase().contains("cellbroadcast")).map(str::to_string)
}

fn is_root(app: &tauri::AppHandle, serial: &str, build_type: &str) -> bool {
    let _ = adb_call(app, &["-s", serial, "root"]);
    thread::sleep(Duration::from_millis(700));
    if shell(app, serial, &["id", "-u"]).map(|s| s.trim()=="0").unwrap_or(false) { return true; }
    if build_type.eq_ignore_ascii_case("eng") || build_type.eq_ignore_ascii_case("userdebug") {
        return shell(app, serial, &["id"]).map(|s| s.contains("uid=0")).unwrap_or(false);
    }
    false
}

fn parse_devices(app: &tauri::AppHandle) -> Result<Vec<Device>, String> {
    let text = adb_call(app, &["devices", "-l"])?;
    let mut result = Vec::new();
    for line in text.lines().skip(1) {
        let trimmed=line.trim();
        if trimmed.is_empty() || trimmed.starts_with('*') { continue; }
        let mut fields=trimmed.split_whitespace();
        let serial=match fields.next(){Some(v)=>v.to_string(),None=>continue};
        let raw=fields.next().unwrap_or("unknown");
        if raw=="unauthorized" {
            result.push(Device{serial,model:None,product:None,manufacturer:None,release:None,sdk:None,build_type:None,debuggable:None,root:false,cellbroadcast_package:None,state:DeviceState::Unauthorized,support_level:SupportLevel::Untested,notes:vec!["Accept the USB debugging authorization prompt on the phone.".into()]});
            continue;
        }
        if raw=="offline" || raw=="no" {
            result.push(Device{serial,model:None,product:None,manufacturer:None,release:None,sdk:None,build_type:None,debuggable:None,root:false,cellbroadcast_package:None,state:DeviceState::Offline,support_level:SupportLevel::Untested,notes:vec!["ADB reports the device as offline.".into()]});
            continue;
        }
        if raw!="device" {
            result.push(Device{serial,model:None,product:None,manufacturer:None,release:None,sdk:None,build_type:None,debuggable:None,root:false,cellbroadcast_package:None,state:DeviceState::Unknown,support_level:SupportLevel::Untested,notes:vec![format!("ADB state: {raw}")]});
            continue;
        }
        let model=getprop(app,&serial,"ro.product.model");
        let product=getprop(app,&serial,"ro.product.device");
        let manufacturer=getprop(app,&serial,"ro.product.manufacturer");
        let release=getprop(app,&serial,"ro.build.version.release");
        let sdk=getprop(app,&serial,"ro.build.version.sdk");
        let build_type=getprop(app,&serial,"ro.build.type");
        let debuggable=getprop(app,&serial,"ro.debuggable");
        let root=is_root(app,&serial,&build_type);
        let cb=find_cellbroadcast(app,&serial);
        let mut notes=Vec::new();
        let samsung_a35 = model.to_ascii_lowercase().contains("sm-a356") || model.to_ascii_lowercase().contains("galaxy a35");
        if samsung_a35 {
            notes.push("Samsung Galaxy A35 detected. Read-only ADB diagnostics are supported on the stock build.".into());
        }
        let (state,support)=if cb.is_none(){notes.push("No CellBroadcast receiver package was detected.".into());(DeviceState::Unsupported,SupportLevel::Unsupported)}
            else if !root{notes.push("Stock/non-root device. The protected CellBroadcast injection path is not demonstrated here.".into());(DeviceState::NoRoot,SupportLevel::RootRequired)}
            else{notes.push("Rooted/userdebug controlled target. Genuine Android CellBroadcast test path is available.".into());(DeviceState::Ready,SupportLevel::Supported)};
        result.push(Device{serial,model:(!model.is_empty()).then_some(model),product:(!product.is_empty()).then_some(product),manufacturer:(!manufacturer.is_empty()).then_some(manufacturer),release:(!release.is_empty()).then_some(release),sdk:(!sdk.is_empty()).then_some(sdk),build_type:(!build_type.is_empty()).then_some(build_type),debuggable:(!debuggable.is_empty()).then_some(debuggable),root,cellbroadcast_package:cb,state,support_level:support,notes});
    }
    Ok(result)
}

fn receiver_prefs_path(pkg:&str)->String{format!("/data/user_de/0/{pkg}/shared_prefs/{pkg}_preferences.xml")}
fn read_receiver_prefs(app:&tauri::AppHandle,serial:&str,pkg:&str)->Result<String,String>{
    let p=receiver_prefs_path(pkg);
    shell(app,serial,&["cat",&p]).or_else(|_|Ok(String::new()))
}
fn flag_enabled(xml:&str,name:&str)->bool{xml.contains(&format!(r#"name="{name}" value="true""#))}
fn update_flag(xml:&str,name:&str)->String{
    let needle=format!(r#"name="{name}""#);
    if xml.contains(&needle){
        let mut out=xml.to_string(); let mut from=0usize;
        while let Some(pos)=out[from..].find(&needle){
            let start=from+pos; let end=start+out[start..].find('>').unwrap_or(0);
            let end=if end<start{start}else{end};
            let segment=out[start..=end].to_string();
            if let Some(v)=segment.find(r#" value=""#){
                let absolute=start+v+r#" value=""#.len();
                if let Some(q)=out[absolute..].find('"'){out.replace_range(absolute..absolute+q,"true");}
            } else if let Some(close)=out[start..=end].rfind("/>"){out.insert_str(start+close, r#" value="true""#);}
            from=end.saturating_add(1);
        }
        return out;
    }
    if let Some(pos)=xml.rfind("</map>"){
        let mut out=xml.to_string();out.insert_str(pos,&format!("  <boolean name=\"{name}\" value=\"true\" />\n"));return out
    }
    format!("<?xml version=\"1.0\" encoding=\"utf-8\" standalone=\"yes\" ?>\n<map>\n  <boolean name=\"{name}\" value=\"true\" />\n</map>\n")
}
fn push_text_file(app:&tauri::AppHandle,serial:&str,remote:&str,text:&str)->Result<(),String>{
    let safe=serial.chars().map(|c|if c.is_ascii_alphanumeric()||c=='-'||c=='_'{c}else{'_'}).collect::<String>();
    let local=std::env::temp_dir().join(format!("emergency-sim-{safe}.xml"));
    fs::write(&local,text).map_err(|e|format!("could not create temporary preferences: {e}"))?;
    let local_s=local.to_string_lossy().to_string();
    let pushed=adb_call(app,&["-s",serial,"push",&local_s,"/data/local/tmp/emergency-sim-prefs.xml"]);
    let _=fs::remove_file(&local);pushed?;
    shell(app,serial,&["sh","-c",&format!("cat /data/local/tmp/emergency-sim-prefs.xml > '{remote}'; rm -f /data/local/tmp/emergency-sim-prefs.xml")]).map(|_|())
}
fn prepare_test_mode(app:&tauri::AppHandle,serial:&str,pkg:&str,dry_run:bool,log:&impl Fn(String))->Result<bool,String>{
    let path=receiver_prefs_path(pkg);let current=read_receiver_prefs(app,serial,pkg)?;
    let testing=flag_enabled(&current,"testing_mode");let enabled=flag_enabled(&current,"enable_test_alerts");
    if testing&&enabled{log("CellBroadcast test mode already enabled".into());return Ok(true)}
    if dry_run{log("Dry run: controlled test prerequisites would be enabled.".into());return Ok(true)}
    let mut updated=current.clone();updated=update_flag(&updated,"testing_mode");updated=update_flag(&updated,"enable_test_alerts");
    log(format!("Preparing controlled test preferences at {path}"));push_text_file(app,serial,&path,&updated)?;
    let _=shell(app,serial,&["am","force-stop",pkg]);thread::sleep(Duration::from_secs(2));
    let verify=read_receiver_prefs(app,serial,pkg)?;
    Ok(flag_enabled(&verify,"testing_mode")&&flag_enabled(&verify,"enable_test_alerts))
}
fn injector_path(app:&tauri::AppHandle)->Option<PathBuf>{
    resource_candidate(app,&["alertinject.jar","android/alertinject/out/alertinject.jar","alertinject/alertinject.jar"])
}
fn push_injector(app:&tauri::AppHandle,serial:&str)->Result<(),String>{
    let jar=injector_path(app).ok_or_else(||"The bundled Android injector is missing from this build.".to_string())?;
    adb_call(app,&["-s",serial,"push",&jar.to_string_lossy(),INJECTOR_REMOTE]).map(|_|())
}
fn collect_evidence(app:&tauri::AppHandle,serial:&str,cancel:&CancelStore,send:&mut SendResult,tx:&TxStore)->Result<(),String>{
    let start=Instant::now();let mut rcv=false;let mut svc=false;let mut audio=false;let mut dialog=false;
    while start.elapsed()<Duration::from_secs(EVIDENCE_TIMEOUT_SECS){
        if cancel_requested(cancel,serial){send.state="CANCELLED".into();send.failure=Some("USER_CANCELLED".into());send.message="Stop requested while waiting for downstream evidence. Outcome is treated as uncertain.".into();tx.map.lock().unwrap().insert(serial.into(),TxState::Uncertain);return Ok(())}
        thread::sleep(Duration::from_secs(2));let dump=shell(app,serial,&["logcat","-d","-t","3000"]).unwrap_or_default();
        if dump.contains("CellBroadcastReceiver")&&!rcv{rcv=true;send.evidence.push("CellBroadcastReceiver.onReceive".into())}
        if dump.contains("CBAlertService")&&!svc{svc=true;send.evidence.push("CellBroadcastAlertService.onStartCommand".into())}
        if dump.contains("CellBroadcastAlertAudio")&&!audio{audio=true;send.evidence.push("CellBroadcastAlertAudio".into())}
        if dump.contains("CellBroadcastAlertDialog")&&!dialog{dialog=true;send.evidence.push("CellBroadcastAlertDialog".into())}
        if dump.contains("Permission Denial: not allowed to send broadcast"){send.state="FAILED".into();send.failure=Some("BROADCAST_REJECTED".into());send.message="Android rejected the protected broadcast before the system alert UI.".into();return Ok(())}
        if dump.contains("ignoring alert of type")||dump.contains("received undefined channels"){send.state="FAILED".into();send.failure=Some("CELLBROADCAST_FILTERED".into());send.message="The CellBroadcast receiver explicitly filtered the test message.".into();return Ok(())}
        if dialog{send.state="ALERT_DISPLAYED".into();send.message="Genuine Android CellBroadcast alert UI was displayed on the device.";tx.map.lock().unwrap().insert(serial.into(),TxState::Delivered);return Ok(())}
    }
    if svc{send.state="RECEIVED_BY_CELLBROADCAST".into();send.failure=Some("TIMEOUT".into());send.message="The CellBroadcast service started, but no system alert UI was observed within the timeout.";tx.map.lock().unwrap().insert(serial.into(),TxState::Uncertain)}
    else if rcv{send.state="FAILED".into();send.failure=Some("ALERT_PROCESSING_FAILED".into());send.message="The receiver saw the message, but no downstream alert service evidence appeared.";tx.map.lock().unwrap().insert(serial.into(),TxState::Uncertain)}
    else{send.state="FAILED".into();send.failure=Some("TIMEOUT".into());send.message="No downstream CellBroadcast evidence was observed. The outcome is uncertain.";tx.map.lock().unwrap().insert(serial.into(),TxState::Uncertain)}
    Ok(())
}

#[tauri::command]
fn app_info(app:tauri::AppHandle)->Result<AppInfo,String>{let(_,source)=adb_path(&app);Ok(AppInfo{version:"2.0.0".into(),adb_source:source,injector:injector_path(&app).is_some()})}
#[tauri::command] fn list_devices(app:tauri::AppHandle)->Result<Vec<Device>,String>{parse_devices(&app)}

#[tauri::command]
fn open_android_settings(app:tauri::AppHandle, serial:String)->Result<(),String>{
    let _ = adb_call(&app, &["-s",&serial,"shell","am","start","-a","android.settings.SETTINGS"])?;
    Ok(())
}
#[tauri::command] fn acknowledge(app:tauri::AppHandle,state:State<SharedTx>,serial:String)->Result<(),String>{set_tx(&app,&state,&serial,None)}
#[tauri::command] fn request_cancel(state:State<SharedCancel>,serial:String)->Result<(),String>{set_cancel(&state,&serial);Ok(())}
#[tauri::command]
async fn send_test_alert(app:tauri::AppHandle,tx:State<'_,SharedTx>,cancel:State<'_,SharedCancel>,serial:String,body:String,dry_run:bool)->Result<SendResult,String>{
    let tx=tx.inner().clone();let cancel=cancel.inner().clone();
    tauri::async_runtime::spawn_blocking(move||{
        let body=body.split_whitespace().collect::<Vec<_>>().join(" ");
        let body=if body.is_empty(){DEFAULT_BODY.to_string()}else{body};
        let mut result=SendResult{device_serial:serial.clone(),category:SERVICE_CATEGORY,body:body.clone(),state:"FAILED".into(),failure:None,message:String::new(),evidence:vec![],injector_exit_code:None};
        if !body.starts_with(REQUIRED_PREFIX){result.failure=Some("INVALID_BODY".into());result.message="The message must begin with TEST.".into();return Ok(result)}
        if body.len()>300{result.failure=Some("INVALID_BODY".into());result.message="The message is longer than 300 characters.".into();return Ok(result)}
        if let Some(reason)=gate_for(&tx,&serial){result.failure=Some("DUPLICATE_SEND_BLOCKED".into());result.message=reason;return Ok(result)}
        emit_log(&app,format!("Inspecting target {serial}"),"info");
        let device=parse_devices(&app)?.into_iter().find(|d|d.serial==serial).ok_or("The selected device is no longer attached.")?;
        if !matches!(device.state,DeviceState::Ready){
            result.failure=Some(match device.state{DeviceState::Unauthorized=>"DEVICE_UNAUTHORIZED",DeviceState::Offline=>"DEVICE_OFFLINE",DeviceState::NoRoot=>"NO_ROOT",DeviceState::Unsupported=>"CELLBROADCAST_MISSING",DeviceState::Unknown=>"DEVICE_UNKNOWN",DeviceState::Ready=>"UNKNOWN"}.into());
            result.message=match device.state{DeviceState::Unauthorized=>"Accept the USB debugging authorization prompt on the phone first.",DeviceState::Offline=>"ADB reports this device as offline.",DeviceState::NoRoot=>"This stock/non-root device cannot use the protected controlled test path.",DeviceState::Unsupported=>"No CellBroadcast receiver package was detected.",DeviceState::Unknown=>"The device is not in a known ADB-ready state.",DeviceState::Ready=>"Device is ready."}.into();
            return Ok(result)
        }
        let pkg=device.cellbroadcast_package.clone().ok_or("No CellBroadcast receiver package was detected.")?;
        emit_log(&app,format!("CellBroadcast receiver found: {pkg}"),"ok");
        if !prepare_test_mode(&app,&serial,&pkg,dry_run,&|m|emit_log(&app,m,"info"))?{result.failure=Some("TEST_MODE_DISABLED".into());result.message="The controlled test-alert preferences could not be established.".into();return Ok(result)}
        if dry_run{result.state="READY_TO_SEND".into();result.message="Dry run passed. No injector was pushed and no message was delivered.".into();emit_log(&app,"Dry run complete · nothing was sent","ok");return Ok(result)}
        if cancel_requested(&cancel,&serial){clear_cancel(&cancel,&serial);result.state="CANCELLED".into();result.failure=Some("USER_CANCELLED".into());result.message="Operation cancelled before delivery.".into();return Ok(result)}
        set_tx(&app,&tx,&serial,Some(TxState::Busy))?;
        let _=adb_call(&app,&["-s",&serial,"logcat","-c"]);
        if let Err(err)=push_injector(&app,&serial){
            let _=set_tx(&app,&tx,&serial,None);
            result.failure=Some("INJECTOR_FAILURE".into());result.message=err;emit_log(&app,result.message.clone(),"error");return Ok(result)
        }
        emit_log(&app,"Injector pushed to the controlled device","ok");
        emit_log(&app,format!("Sending ETWS TEST · category {SERVICE_CATEGORY}"),"info");
        let cat=SERVICE_CATEGORY.to_string();
        let out=command_output(&app,&["-s",&serial,"shell","CLASSPATH=/data/local/tmp/alertinject.jar","app_process","/system/bin",INJECTOR_CLASS,&cat,&body])?;
        result.injector_exit_code=out.status.code();let text=output_text(&out);
        if let Some(first)=text.lines().next(){emit_log(&app,format!("Injector: {first}"),"info")}
        collect_evidence(&app,&serial,&cancel,&mut result,&tx)?;
        if result.state=="FAILED" && result.failure.as_deref()==Some("BROADCAST_REJECTED"){let _=set_tx(&app,&tx,&serial,None)}
        let _=persist_transactions(&app,&tx);
        clear_cancel(&cancel,&serial);
        emit_log(&app,format!("Result: {}",result.state),if result.state=="ALERT_DISPLAYED"{"ok"}else{"warn"});
        Ok(result)
    }).await.map_err(|e|format!("controller worker failed: {e}"))?
}
#[tauri::command] fn window_minimize(window:tauri::Window)->Result<(),String>{window.minimize().map_err(|e|e.to_string())}
#[tauri::command] fn window_toggle_maximize(window:tauri::Window)->Result<(),String>{if window.is_maximized().map_err(|e|e.to_string())?{window.unmaximize().map_err(|e|e.to_string())}else{window.maximize().map_err(|e|e.to_string())}}
#[tauri::command] fn window_close(window:tauri::Window)->Result<(),String>{window.close().map_err(|e|e.to_string())}
#[tauri::command] fn reset_safety_state(app:tauri::AppHandle,state:State<SharedTx>)->Result<(),String>{*state.error.lock().unwrap()=None;*state.map.lock().unwrap()=HashMap::new();let _=fs::remove_file(tx_path(&app)?);Ok(())}

pub fn run(){
 let tx:SharedTx=Arc::new(TxStore::default());let cancel:SharedCancel=Arc::new(CancelStore::default());
 tauri::Builder::default().manage(tx.clone()).manage(cancel).setup(move|app|{load_transactions(app.handle(),&tx);emit_log(app.handle(),"Emergency Simulator desktop runtime started","ok");Ok(())})
 .invoke_handler(tauri::generate_handler![app_info,list_devices,send_test_alert,acknowledge,request_cancel,reset_safety_state,open_android_settings,window_minimize,window_toggle_maximize,window_close])
 .run(tauri::generate_context!()).expect("error while running Emergency Simulator");
}
