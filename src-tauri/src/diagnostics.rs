//! Structured device diagnostics for the Cell Broadcast investigation.
//!
//! The controller's job on a stock phone is to find out what the phone actually does, not to assert
//! what it ought to do. This module is the part that turns raw `adb` output into a machine-readable
//! report, and it is written so that every value is either read from the device or explicitly
//! reported as unknown. There is no default that flatters the result.
//!
//! Two rules run through the whole file:
//!
//! * **A command that failed is not a negative answer.** If `dumpsys package` returns an empty
//!   stdout, the permission state is `Unknown`, never `Denied`. An earlier revision of this project
//!   told an operator their notification permission was denied when it was granted; the fix was to
//!   stop collapsing "could not read" into "no".
//! * **A package name is not a component.** Discovery classifies every package that could plausibly
//!   participate (telephony, emergency, carrier config, SystemUI, Samsung framework), and records
//!   the reason it was included. Name matching alone is a hint that decides what to inspect, never a
//!   conclusion.
//!
//! The parsers are pure functions over text so they can be tested against fixture dumps from both
//! AOSP and Samsung firmware without a device present.

use serde::Serialize;

/// How strongly a package is implicated in emergency Cell Broadcast delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Relevance {
    /// A named Cell Broadcast component: the receiver or the service module.
    CellBroadcastComponent,
    /// Emergency-alert presentation or telephony transport, but not the CB component itself.
    EmergencyOrTelephony,
    /// Carrier configuration, which decides which channels are enabled and displayable.
    CarrierConfig,
    /// Matched a keyword but has no indicated role. Recorded, not prioritised.
    KeywordMatch,
}

impl Relevance {
    pub fn label(self) -> &'static str {
        match self {
            Relevance::CellBroadcastComponent => "CELL_BROADCAST_COMPONENT",
            Relevance::EmergencyOrTelephony => "EMERGENCY_OR_TELEPHONY",
            Relevance::CarrierConfig => "CARRIER_CONFIG",
            Relevance::KeywordMatch => "KEYWORD_MATCH",
        }
    }
}

/// One package worth recording, with the reason it was recorded.
///
/// `matched_on` is deliberately explicit: a future reader must be able to see whether a package was
/// included because it is the Cell Broadcast receiver or because its name happened to contain
/// `alert`. Those are different strengths of evidence and must not be presented alike.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackageRecord {
    pub name: String,
    pub relevance: Relevance,
    pub matched_on: Vec<String>,
    /// APK paths from `pm list packages -f`, so an APEX-shipped module is visible as such.
    pub code_paths: Vec<String>,
    /// The UID, when the dump reported one.
    pub uid: Option<u32>,
    /// Permission names the package *requested*.
    pub requested_permissions: Vec<String>,
    /// Permission names reported granted.
    pub granted_permissions: Vec<String>,
    /// Permission names explicitly denied. Only ever populated from an explicit denial.
    pub denied_permissions: Vec<String>,
    /// `dumpsys package` receiver blocks that look like Cell Broadcast receivers.
    pub cellbroadcast_receivers: Vec<ReceiverRecord>,
    /// Whether the package dump was actually readable. False drives `UNKNOWN`, not `DENIED`.
    pub dump_readable: bool,
}

/// A receiver declaration found in a package dump.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReceiverRecord {
    /// The component name as printed, e.g. `com.x/.CellBroadcastReceiver`.
    pub component: String,
    /// Intent actions the receiver's filters declare.
    pub actions: Vec<String>,
    /// Whether any non-system caller may target it. `None` when the dump did not say.
    pub exported: Option<bool>,
    /// Whether the component is enabled, when the dump said so.
    pub enabled: Option<bool>,
}

/// Keywords, grouped by the role they indicate. Order matters: the first family that matches decides
/// the relevance, so the strongest signal is listed first.
const CELL_BROADCAST_KEYWORDS: &[&str] = &["cellbroadcast", "cell_broadcast", "smscb", "sms_cb"];
const EMERGENCY_TELEPHONY_KEYWORDS: &[&str] = &[
    "emergency",
    "etws",
    "cmas",
    "wirelessalert",
    "wireless_alert",
    "telephony",
    "telecom",
    "ril",
    "oplusril",
    "systemui",
    "framework",
];
const CARRIER_CONFIG_KEYWORDS: &[&str] = &["carrierconfig", "carrier_config", "carrier"];

/// Decide whether a package belongs in the report, and why.
///
/// Returns `None` for packages with no indicated role. The check is intentionally broader than
/// `contains("cellbroadcast")`, because the component that renders the alert and the configuration
/// that decides which channels are displayable are both relevant and neither is named after Cell
/// Broadcast.
pub fn classify_package(package_name: &str) -> Option<(Relevance, Vec<String>)> {
    let lower = package_name.to_ascii_lowercase();

    let matched = |keywords: &[&str]| -> Vec<String> {
        keywords
            .iter()
            .filter(|keyword| lower.contains(**keyword))
            .map(|keyword| (*keyword).to_string())
            .collect()
    };

    let cb = matched(CELL_BROADCAST_KEYWORDS);
    if !cb.is_empty() {
        return Some((Relevance::CellBroadcastComponent, cb));
    }
    let telephony = matched(EMERGENCY_TELEPHONY_KEYWORDS);
    if !telephony.is_empty() {
        return Some((Relevance::EmergencyOrTelephony, telephony));
    }
    let carrier = matched(CARRIER_CONFIG_KEYWORDS);
    if !carrier.is_empty() {
        return Some((Relevance::CarrierConfig, carrier));
    }

    // A second, weaker pass: Samsung ships emergency UI inside packages whose names carry none of
    // the words above, so an explicit Samsung-framework check is recorded as a keyword match rather
    // than silently dropped.
    let weak = ["samsung", "sec.android", "sec.telephony", "alert", "disaster"];
    let weak_matched = matched(&weak);
    if !weak_matched.is_empty() {
        return Some((Relevance::KeywordMatch, weak_matched));
    }

    None
}

/// Parse `pm list packages -f` output into name/APK-path pairs.
///
/// The format is `package:/path/to/base.apk=com.example.app`. Split on the **first** `=` after the
/// path, because an APK path can contain `=` and a package name cannot.
pub fn parse_pm_list_packages_f(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("package:")?;
            let (path, name) = rest.rsplit_once('=')?;
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            Some((name.to_string(), path.trim().to_string()))
        })
        .collect()
}

/// Parse a `getprop` batch in the `[key]: [value]` form.
///
/// Values are returned exactly as printed, including empty strings, because an empty property that
/// was read is different from a property that was never read.
pub fn parse_getprop_batch(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix('[')?;
            let (key, rest) = rest.split_once("]: [")?;
            let value = rest.strip_suffix(']')?;
            Some((key.to_string(), value.to_string()))
        })
        .collect()
}

/// Parse the `requested permissions:` block of a `dumpsys package` dump.
fn parse_requested_permissions(dump: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_block = false;
    for line in dump.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("requested permissions:") {
            in_block = true;
            continue;
        }
        if in_block {
            if trimmed.is_empty() {
                continue;
            }
            // The block ends at the next section header, which is not indented as a permission.
            if trimmed.ends_with(':') && !trimmed.starts_with("android.permission") {
                break;
            }
            // Runtime/granted blocks are separate sections.
            if trimmed.starts_with("install permissions:")
                || trimmed.starts_with("runtime permissions:")
                || trimmed.starts_with("declared permissions:")
                || trimmed.starts_with("User 0:")
            {
                break;
            }
            if trimmed.starts_with("android.permission.") || trimmed.contains('.') {
                out.push(trimmed.to_string());
            }
        }
    }
    out
}

/// Parse granted and denied runtime/install permissions.
///
/// The return is `(granted, denied)`. A permission is only placed in `denied` when the dump says
/// `granted=false` for it. A permission that appears with no state, or that never appears at all,
/// goes in neither list — that is the whole point, and it is what the earlier bug got wrong.
pub fn parse_permission_states(dump: &str) -> (Vec<String>, Vec<String>) {
    let mut granted = Vec::new();
    let mut denied = Vec::new();
    let mut in_section = false;

    for line in dump.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("runtime permissions:")
            || trimmed.starts_with("install permissions:")
            || trimmed.starts_with("declared permissions:")
        {
            in_section = true;
            continue;
        }
        if !in_section {
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        if !trimmed.starts_with("android.permission.") {
            in_section = false;
            continue;
        }
        // `android.permission.X: granted=true, flags=[...]` or `android.permission.X: granted=false`
        let (name, rest) = match trimmed.split_once(':') {
            Some(parts) => parts,
            None => continue,
        };
        let name = name.trim().to_string();
        match parse_granted_flag(rest) {
            Some(true) => granted.push(name),
            Some(false) => denied.push(name),
            None => {}
        }
    }

    (granted, denied)
}

/// Read `granted=true|false` out of a permission line's tail. `None` when the line does not say.
fn parse_granted_flag(text: &str) -> Option<bool> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("granted=true") {
        Some(true)
    } else if lower.contains("granted=false") {
        Some(false)
    } else {
        None
    }
}

/// Parse receiver declarations out of a package dump.
///
/// `dumpsys package` prints receiver blocks as:
///
/// ```text
///   Receiver #0:
///     com.x/.CellBroadcastReceiver
///       Actions:
///         android.provider.action.SMS_EMERGENCY_CB_RECEIVED
///       enabled=true
///       exported=true
/// ```
///
/// Indentation is not guaranteed to be stable across Android versions, so this tracks the current
/// component by the shape of the line rather than by column offsets.
pub fn parse_receivers(dump: &str) -> Vec<ReceiverRecord> {
    let mut receivers: Vec<ReceiverRecord> = Vec::new();
    let mut current: Option<ReceiverRecord> = None;
    let mut in_actions = false;

    let flush = |current: &mut Option<ReceiverRecord>, out: &mut Vec<ReceiverRecord>| {
        if let Some(record) = current.take() {
            out.push(record);
        }
    };

    for line in dump.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("Receiver #") || trimmed.starts_with("Receiver:") {
            flush(&mut current, &mut receivers);
            in_actions = false;
            continue;
        }

        // A component line right after a `Receiver #` header: `package/.Receiver` or
        // `package/package.Receiver`, optionally with a leading `android.intent.action` marker.
        let looks_like_component = trimmed.contains('/')
            && !trimmed.starts_with("filter ")
            && !trimmed.contains(' ')
            && !trimmed.starts_with("Scheme:");

        if looks_like_component && current.is_none() {
            current = Some(ReceiverRecord {
                component: trimmed.to_string(),
                actions: Vec::new(),
                exported: None,
                enabled: None,
            });
            in_actions = false;
            continue;
        }

        if current.is_none() {
            continue;
        }

        if trimmed == "Actions:" {
            in_actions = true;
            continue;
        }

        if trimmed.starts_with("exported=") {
            in_actions = false;
            if let Some(value) = parse_bool_after(trimmed, "exported=") {
                if let Some(record) = current.as_mut() {
                    record.exported = Some(value);
                }
            }
            continue;
        }

        if trimmed.starts_with("enabled=") || trimmed.starts_with("state=") {
            let key = if trimmed.starts_with("enabled=") { "enabled=" } else { "state=" };
            if let Some(value) = parse_bool_after(trimmed, key) {
                if let Some(record) = current.as_mut() {
                    record.enabled = Some(value);
                }
            }
            if trimmed.starts_with("enabled=") {
                in_actions = false;
            }
            continue;
        }

        if in_actions {
            // Filter blocks print the action on its own line. A filter header ends the list.
            if trimmed.starts_with("filter ") || trimmed.ends_with(':') {
                in_actions = false;
                continue;
            }
            if trimmed.contains('.') && !trimmed.contains(' ') {
                if let Some(record) = current.as_mut() {
                    record.actions.push(trimmed.to_string());
                }
            }
        }
    }

    flush(&mut current, &mut receivers);
    receivers
}

fn parse_bool_after(line: &str, key: &str) -> Option<bool> {
    let rest = line.strip_prefix(key)?;
    let token = rest.split([',', ' ', ']', ';']).next()?.trim();
    match token {
        "true" | "enabled" | "ENABLED" => Some(true),
        "false" | "disabled" | "DISABLED" => Some(false),
        _ => None,
    }
}

/// The receiver is the one this project cares about if its actions or name say Cell Broadcast.
pub fn receiver_is_cellbroadcast(receiver: &ReceiverRecord) -> bool {
    let name = receiver.component.to_ascii_lowercase();
    if name.contains("cellbroadcast") {
        return true;
    }
    receiver.actions.iter().any(|action| {
        let action = action.to_ascii_lowercase();
        action.contains("sms_cb_received")
            || action.contains("sms_emergency_cb_received")
            || action.contains("cellbroadcast")
    })
}

/// Build one `PackageRecord` from raw command output.
pub fn package_record(
    name: &str,
    code_paths: Vec<String>,
    dump: &str,
) -> Option<PackageRecord> {
    let (relevance, matched_on) = classify_package(name)?;
    let dump_readable = !dump.trim().is_empty();

    let (uid, requested, granted, denied, receivers) = if dump_readable {
        (
            parse_uid(dump),
            parse_requested_permissions(dump),
            parse_permission_states(dump).0,
            parse_permission_states(dump).1,
            parse_receivers(dump)
                .into_iter()
                .filter(receiver_is_cellbroadcast)
                .collect(),
        )
    } else {
        (None, Vec::new(), Vec::new(), Vec::new(), Vec::new())
    };

    Some(PackageRecord {
        name: name.to_string(),
        relevance,
        matched_on,
        code_paths,
        uid,
        requested_permissions: requested,
        granted_permissions: granted,
        denied_permissions: denied,
        cellbroadcast_receivers: receivers,
        dump_readable,
    })
}

/// Read the UID out of a `dumpsys package` header line: `Package [com.x] (1a2b3c):` followed by
/// `userId=10123` in the body.
pub fn parse_uid(dump: &str) -> Option<u32> {
    for line in dump.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("userId=") {
            if let Some(value) = rest.split_whitespace().next() {
                if let Ok(uid) = value.parse::<u32>() {
                    return Some(uid);
                }
            }
        }
    }
    None
}

/// The overall machine-readable picture, matching the shape the investigation brief asks for.
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticReport {
    pub device: DeviceSnapshot,
    pub cell_broadcast: CellBroadcastPicture,
    /// Every command that was run for this report, with its raw result.
    pub commands: Vec<CommandResult>,
}

/// Read-only device facts, as strings because a property that is unset must stay visibly unset.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DeviceSnapshot {
    pub model: Option<String>,
    pub device: Option<String>,
    pub manufacturer: Option<String>,
    pub android_release: Option<String>,
    pub sdk: Option<String>,
    pub build_id: Option<String>,
    pub build_display_id: Option<String>,
    pub build_incremental: Option<String>,
    pub build_type: Option<String>,
    pub build_tags: Option<String>,
    pub build_fingerprint: Option<String>,
    pub security_patch: Option<String>,
    /// `ro.debuggable`. The single property that decides whether the AOSP test receiver exists.
    pub debuggable: Option<String>,
    pub secure: Option<String>,
    pub abi: Option<String>,
    pub kernel: Option<String>,
    pub verified_boot_state: Option<String>,
}

impl DeviceSnapshot {
    pub fn from_properties(properties: &[(String, String)]) -> Self {
        let get = |key: &str| -> Option<String> {
            properties
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
                .filter(|v| !v.is_empty())
        };
        Self {
            model: get("ro.product.model"),
            device: get("ro.product.device"),
            manufacturer: get("ro.product.manufacturer"),
            android_release: get("ro.build.version.release"),
            sdk: get("ro.build.version.sdk"),
            build_id: get("ro.build.id"),
            build_display_id: get("ro.build.display.id"),
            build_incremental: get("ro.build.version.incremental"),
            build_type: get("ro.build.type"),
            build_tags: get("ro.build.tags"),
            build_fingerprint: get("ro.build.fingerprint"),
            security_patch: get("ro.build.version.security_patch"),
            debuggable: get("ro.debuggable"),
            secure: get("ro.secure"),
            abi: get("ro.product.cpu.abi"),
            kernel: get("ro.kernel.version").or_else(|| get("ro.boot.kernel")),
            verified_boot_state: get("ro.boot.verifiedbootstate"),
        }
    }
}

/// What the investigation needs to know about the Cell Broadcast surface.
///
/// Each field is a fact read from the device. `test_entrypoint_accepted` is deliberately absent:
/// that is established by an experiment, and is recorded in the experiment journal rather than
/// asserted here.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CellBroadcastPicture {
    /// The Cell Broadcast component package, if one was found.
    pub package_present: bool,
    pub package_name: Option<String>,
    /// Whether that package's dump showed a Cell Broadcast receiver declaration.
    pub receiver_discovered: bool,
    /// Whether the build property permits the AOSP test receiver to exist at all.
    pub test_entrypoint_discovered: bool,
    /// Whether the receiver declares the emergency action, which is the protected one.
    pub declares_emergency_action: bool,
    /// The precise reason, so the report never has to be interpreted against a bare boolean.
    pub reason: String,
}

/// One command and everything it returned. Nothing is dropped.
#[derive(Debug, Clone, Serialize)]
pub struct CommandResult {
    pub label: String,
    pub command: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// What the collector made of the output, in one line.
    pub parsed: String,
}

/// Assemble the report from the already-captured command results.
///
/// This is a pure function over the captured output so the whole report can be built and asserted in
/// a test from fixture text, with no device and no process spawning.
pub fn build_report(
    properties: &[(String, String)],
    packages: &[PackageRecord],
    commands: Vec<CommandResult>,
) -> DiagnosticReport {
    let device = DeviceSnapshot::from_properties(properties);

    let cb_package = packages
        .iter()
        .find(|package| package.relevance == Relevance::CellBroadcastComponent);

    let receiver_discovered = cb_package
        .map(|package| !package.cellbroadcast_receivers.is_empty())
        .unwrap_or(false);

    let declares_emergency_action = cb_package
        .map(|package| {
            package.cellbroadcast_receivers.iter().any(|receiver| {
                receiver.actions.iter().any(|action| {
                    let action = action.to_ascii_lowercase();
                    action.contains("sms_emergency_cb_received")
                })
            })
        })
        .unwrap_or(false);

    let debuggable = device.debuggable.clone().unwrap_or_default();
    let test_entrypoint_discovered = crate::platform::debuggable_permits_test_entrypoint(&debuggable)
        .unwrap_or(false);

    let reason = match (cb_package, receiver_discovered, test_entrypoint_discovered) {
        (None, _, _) => {
            "No Cell Broadcast component package was found on this device. Nothing in the report \
             was inferred from a package name."
                .to_string()
        }
        (Some(package), false, _) => format!(
            "{} is present but its dump did not show a Cell Broadcast receiver declaration, or the \
             dump could not be read. The receiver is not confirmed.",
            package.name
        ),
        (Some(package), true, false) => format!(
            "{} declares a Cell Broadcast receiver, but ro.debuggable={} means this is a production \
             build. The AOSP test receiver inside GsmInboundSmsHandler is never registered, so a \
             test broadcast is accepted by `am` and then discarded. This is a build property and \
             cannot be granted on the device.",
            package.name,
            if debuggable.is_empty() { "?" } else { &debuggable }
        ),
        (Some(package), true, true) => format!(
            "{} declares a Cell Broadcast receiver and ro.debuggable=1, so the AOSP test receiver \
             exists. Whether a test broadcast reaches the telephony pipeline is an experiment, not \
             a conclusion, and must be established from downstream logcat evidence.",
            package.name
        ),
    };

    DiagnosticReport {
        cell_broadcast: CellBroadcastPicture {
            package_present: cb_package.is_some(),
            package_name: cb_package.map(|package| package.name.clone()),
            receiver_discovered,
            test_entrypoint_discovered,
            declares_emergency_action,
            reason,
        },
        device,
        commands,
    }
}

/// Render the report as Markdown an operator can read and attach to an issue.
///
/// The structure is deliberate. It opens with what is known, unknown, verified and not verified,
/// because the failure mode this project exists to prevent is a reader taking an inference for a
/// measurement. Every section states the evidence for its claim, and the commands section is
/// included verbatim so the reader can re-check anything.
pub fn render_report(report: &DiagnosticReport) -> String {
    let mut out = String::new();
    let device = &report.device;
    let cb = &report.cell_broadcast;

    let value = |option: &Option<String>| -> String {
        match option {
            Some(text) if !text.is_empty() => text.clone(),
            _ => "not read".to_string(),
        }
    };

    out.push_str("# Device diagnostic report\n\n");

    out.push_str("## What we know\n\n");
    out.push_str(&format!(
        "Read from the device in this session:\n\n\
         | Property | Value |\n| --- | --- |\n\
         | Model | {} |\n\
         | Device | {} |\n\
         | Manufacturer | {} |\n\
         | Android | {} (SDK {}) |\n\
         | Build type | {} |\n\
         | Build tags | {} |\n\
         | Build fingerprint | {} |\n\
         | Security patch | {} |\n\
         | ro.debuggable | {} |\n\
         | ro.secure | {} |\n\
         | ABI | {} |\n\
         | Verified boot state | {} |\n\n",
        value(&device.model),
        value(&device.device),
        value(&device.manufacturer),
        value(&device.android_release),
        value(&device.sdk),
        value(&device.build_type),
        value(&device.build_tags),
        value(&device.build_fingerprint),
        value(&device.security_patch),
        value(&device.debuggable),
        value(&device.secure),
        value(&device.abi),
        value(&device.verified_boot_state),
    ));

    out.push_str("## What we verified\n\n");
    out.push_str(&format!(
        "- Cell Broadcast component package present: **{}**{}\n",
        cb.package_present,
        cb.package_name
            .as_ref()
            .map(|name| format!(" (`{name}`)"))
            .unwrap_or_default()
    ));
    out.push_str(&format!(
        "- Cell Broadcast receiver declared in that package: **{}**\n",
        cb.receiver_discovered
    ));
    out.push_str(&format!(
        "- Receiver declares the protected emergency action: **{}**\n",
        cb.declares_emergency_action
    ));
    out.push_str(&format!(
        "- Build permits the AOSP test entry point (`ro.debuggable=1`): **{}**\n\n",
        cb.test_entrypoint_discovered
    ));
    out.push_str(&format!("{}\n\n", cb.reason));

    // The entry-point surface is a fact about Android, not about this device, so it is stated as
    // verified here and enumerated row by row. It is the answer to "did we check everywhere?".
    out.push_str("## The native test surface, enumerated\n\n");
    out.push_str(&format!("{}\n\n", crate::platform::entry_point_summary()));
    out.push_str("| Component | Action | Outcome |\n| --- | --- | --- |\n");
    for entry in crate::platform::entry_points() {
        out.push_str(&format!(
            "| {} | `{}` | {} — {} |\n",
            entry.component,
            entry.action,
            entry.outcome.label(),
            entry.outcome.explain()
        ));
    }
    out.push('\n');

    // The Samsung-specific surface, read out of a firmware image rather than from the device. It is
    // printed with its provenance so a "PROVEN (firmware)" row can never be read as a device
    // observation — this report only reads state from the attached phone.
    out.push_str("## The native Samsung surface, read from firmware\n\n");
    out.push_str(&format!("{}\n\n", crate::platform::samsung_native_entrypoint_conclusion()));
    out.push_str("| Subject | Claim | Evidence | Reference |\n| --- | --- | --- | --- |\n");
    for fact in crate::platform::samsung_firmware_facts() {
        out.push_str(&format!(
            "| {} | {} | {} | `{}` |\n",
            fact.subject,
            fact.claim,
            fact.evidence.label(),
            fact.reference
        ));
    }
    out.push('\n');
    out.push_str(
        "This is an image analysis, not a reading of the attached phone. A row labelled \
         `PROVEN (firmware)` refers to the named build and would need re-reading if the phone has \
         taken a different update.\n\n",
    );

    out.push_str("## What we could not verify\n\n");
    out.push_str(
        "- Whether a test broadcast actually reaches the telephony pipeline. That is an experiment \
         on the device, and this report only reads state. Absence of evidence here is not evidence \
         that the path is closed.\n",
    );
    out.push_str(
        "- Whether the attached phone's build matches the analyzed firmware. The firmware facts are \
         labelled with the exact build they came from; matching this phone to it is a separate step.\n",
    );
    out.push_str(
        "- Carrier configuration: which Cell Broadcast channels are enabled and marked displayable. \
         A message on a disabled channel is accepted and discarded without an alert.\n\n",
    );

    out.push_str("## Commands run\n\n");
    out.push_str(
        "Every command below was executed read-only on the device. Exit code, stdout and stderr are \
         recorded as returned.\n\n",
    );
    for command in &report.commands {
        out.push_str(&format!("### {}\n\n", command.label));
        out.push_str(&format!("Command: `{}`\n\n", command.command));
        out.push_str(&format!(
            "Exit code: {}\n\nInterpretation: {}\n\n",
            command
                .exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "not reported".to_string()),
            command.parsed
        ));
        if !command.stdout.trim().is_empty() {
            out.push_str("stdout:\n\n```\n");
            out.push_str(command.stdout.trim_end());
            out.push_str("\n```\n\n");
        } else {
            out.push_str("stdout: *(nothing)*\n\n");
        }
        if !command.stderr.trim().is_empty() {
            out.push_str("stderr:\n\n```\n");
            out.push_str(command.stderr.trim_end());
            out.push_str("\n```\n\n");
        }
    }

    out.push_str("---\n\n");
    out.push_str(
        "Generated by the Emergency Simulator desktop controller. Read-only: no setting was \
         written, nothing was installed, and no Cell Broadcast was transmitted.\n",
    );

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------------------------
    // Package discovery: the brief's "do not search only for cellbroadcast"
    // -------------------------------------------------------------------------------------------

    #[test]
    fn the_receiver_package_is_classified_as_the_component() {
        let (relevance, matched) = classify_package("com.google.android.cellbroadcastreceiver").unwrap();
        assert_eq!(relevance, Relevance::CellBroadcastComponent);
        assert!(matched.iter().any(|m| m == "cellbroadcast"));
    }

    #[test]
    fn a_samsung_cell_broadcast_package_is_not_missed() {
        let (relevance, _) = classify_package("com.samsung.android.cellbroadcastreceiver").unwrap();
        assert_eq!(relevance, Relevance::CellBroadcastComponent);
    }

    #[test]
    fn a_dump_shows_an_apex_shipped_module() {
        let rows = parse_pm_list_packages_f(
            "package:/apex/com.android.cellbroadcast/priv-app/GoogleCellBroadcastApp@350820300/GoogleCellBroadcastApp.apk=com.google.android.cellbroadcastreceiver\n\
             package:/system/priv-app/SystemUI/SystemUI.apk=com.android.systemui",
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "com.google.android.cellbroadcastreceiver");
        assert!(rows[0].1.contains("com.android.cellbroadcast"));
        assert_eq!(rows[1].0, "com.android.systemui");
    }

    /// The point of the broader classifier: the package that presents the alert and the package
    /// that decides which channels are displayable are both relevant, and neither is named after
    /// Cell Broadcast.
    #[test]
    fn alert_presentation_and_carrier_config_are_included_with_a_reason() {
        let systemui = classify_package("com.android.systemui").unwrap();
        assert_eq!(systemui.0, Relevance::EmergencyOrTelephony);
        assert!(systemui.1.contains(&"systemui".to_string()));

        let carrier = classify_package("com.android.carrierconfig").unwrap();
        assert_eq!(carrier.0, Relevance::CarrierConfig);
    }

    #[test]
    fn an_unrelated_package_is_excluded_entirely() {
        assert!(classify_package("com.android.chrome").is_none());
        assert!(classify_package("com.spotify.music").is_none());
    }

    // -------------------------------------------------------------------------------------------
    // Permission parsing: an unreadable dump must never become a denial
    // -------------------------------------------------------------------------------------------

    #[test]
    fn a_granted_runtime_permission_is_granted() {
        let dump = "\
Package [com.tirodz.emergencysimulator] (1a2b3c):
    userId=10123
  requested permissions:
    android.permission.POST_NOTIFICATIONS
    android.permission.VIBRATE
  runtime permissions:
    android.permission.POST_NOTIFICATIONS: granted=true, flags=[ USER_SET ]
    android.permission.VIBRATE: granted=true
";
        let (granted, denied) = parse_permission_states(dump);
        assert!(granted.contains(&"android.permission.POST_NOTIFICATIONS".to_string()));
        assert!(granted.contains(&"android.permission.VIBRATE".to_string()));
        assert!(denied.is_empty(), "a granted permission must not appear as denied");
    }

    /// The exact defect the operator hit: a requested permission with no state was read as a denial.
    #[test]
    fn a_requested_permission_with_no_state_is_neither_granted_nor_denied() {
        let dump = "\
Package [com.tirodz.emergencysimulator] (1a2b3c):
  requested permissions:
    android.permission.POST_NOTIFICATIONS
";
        let (granted, denied) = parse_permission_states(dump);
        assert!(granted.is_empty());
        assert!(
            denied.is_empty(),
            "an unstated permission must not be reported as denied"
        );
        let requested = parse_requested_permissions(dump);
        assert_eq!(requested, vec!["android.permission.POST_NOTIFICATIONS".to_string()]);
    }

    #[test]
    fn an_explicit_denial_is_a_denial() {
        let dump = "\
Package [com.x] (1a2b):
  runtime permissions:
    android.permission.POST_NOTIFICATIONS: granted=false, flags=[ USER_SET ]
";
        let (granted, denied) = parse_permission_states(dump);
        assert!(granted.is_empty());
        assert_eq!(denied, vec!["android.permission.POST_NOTIFICATIONS".to_string()]);
    }

    #[test]
    fn an_empty_dump_yields_no_permission_claims_at_all() {
        let (granted, denied) = parse_permission_states("");
        assert!(granted.is_empty() && denied.is_empty());
        assert_eq!(parse_uid(""), None);
    }

    #[test]
    fn the_uid_is_read_from_the_dump_body() {
        let dump = "Package [com.google.android.cellbroadcastreceiver] (abc):\n  userId=10123\n";
        assert_eq!(parse_uid(dump), Some(10123));
    }

    // -------------------------------------------------------------------------------------------
    // Receiver parsing, including the emergency action
    // -------------------------------------------------------------------------------------------

    const AOSP_RECEIVER_DUMP: &str = r#"
Package [com.google.android.cellbroadcastreceiver] (9f2a1):
    userId=10123
    pkgFlags=[ SYSTEM HAS_CODE ]
  requested permissions:
    android.permission.RECEIVE_EMERGENCY_BROADCAST
  Receiver Resolver Table:
    com.google.android.cellbroadcastreceiver/.CellBroadcastReceiver:
      Actions:
        android.provider.Telephony.SMS_CB_RECEIVED
        android.provider.action.SMS_EMERGENCY_CB_RECEIVED
        android.provider.Telephony.SMS_SERVICE_CATEGORY_PROGRAM_DATA_RECEIVED
      exported=true
      enabled=true
"#;

    #[test]
    fn the_emergency_action_is_detected_from_a_receiver_block() {
        let receivers = parse_receivers(AOSP_RECEIVER_DUMP);
        let cb: Vec<_> = receivers
            .iter()
            .filter(|receiver| receiver_is_cellbroadcast(receiver))
            .collect();
        assert_eq!(cb.len(), 1, "expected exactly the CellBroadcast receiver");
        assert!(cb[0].component.contains("CellBroadcastReceiver"));
        assert!(cb[0]
            .actions
            .iter()
            .any(|action| action.contains("SMS_EMERGENCY_CB_RECEIVED")));
        assert_eq!(cb[0].exported, Some(true));
        assert_eq!(cb[0].enabled, Some(true));
    }

    #[test]
    fn a_receiver_that_is_not_cellbroadcast_is_filtered_out() {
        let receivers = parse_receivers(
            "  Receiver Resolver Table:\n\
             com.x/.BootReceiver:\n      Actions:\n        android.intent.action.BOOT_COMPLETED\n      exported=false\n",
        );
        assert!(receivers
            .iter()
            .all(|receiver| !receiver_is_cellbroadcast(receiver)));
    }

    // -------------------------------------------------------------------------------------------
    // Report assembly: the decision the whole module exists to make
    // -------------------------------------------------------------------------------------------

    fn cb_package_with_receiver() -> PackageRecord {
        package_record(
            "com.google.android.cellbroadcastreceiver",
            vec!["/apex/com.android.cellbroadcast/priv-app/GoogleCellBroadcastApp.apk".to_string()],
            AOSP_RECEIVER_DUMP,
        )
        .unwrap()
    }

    #[test]
    fn a_retail_build_is_reported_as_a_build_boundary_not_a_missing_receiver() {
        let properties = vec![
            ("ro.debuggable".to_string(), "0".to_string()),
            ("ro.build.type".to_string(), "user".to_string()),
            ("ro.product.model".to_string(), "SM-A356B".to_string()),
        ];
        let report = build_report(&properties, &[cb_package_with_receiver()], Vec::new());

        assert!(report.cell_broadcast.package_present);
        assert!(report.cell_broadcast.receiver_discovered);
        assert!(report.cell_broadcast.declares_emergency_action);
        assert!(
            !report.cell_broadcast.test_entrypoint_discovered,
            "ro.debuggable=0 must not be reported as permitting the test entry point"
        );
        assert!(
            report.cell_broadcast.reason.contains("ro.debuggable=0"),
            "the reason must name the property that blocks it: {}",
            report.cell_broadcast.reason
        );
        assert!(report.cell_broadcast.reason.contains("cannot be granted"));
        assert_eq!(report.device.model.as_deref(), Some("SM-A356B"));
        assert_eq!(report.device.build_type.as_deref(), Some("user"));
    }

    #[test]
    fn a_userdebug_build_reports_the_entry_point_as_discovered_but_not_proven() {
        let properties = vec![("ro.debuggable".to_string(), "1".to_string())];
        let report = build_report(&properties, &[cb_package_with_receiver()], Vec::new());
        assert!(report.cell_broadcast.test_entrypoint_discovered);
        assert!(
            report.cell_broadcast.reason.contains("an experiment, not"),
            "must not claim a trial that was never run: {}",
            report.cell_broadcast.reason
        );
    }

    #[test]
    fn an_unreadable_package_dump_does_not_produce_a_receiver_claim() {
        let package = package_record("com.samsung.android.cellbroadcastreceiver", Vec::new(), "")
            .unwrap();
        assert!(!package.dump_readable);

        let properties = vec![("ro.debuggable".to_string(), "0".to_string())];
        let report = build_report(&properties, &[package], Vec::new());
        assert!(report.cell_broadcast.package_present);
        assert!(
            !report.cell_broadcast.receiver_discovered,
            "an unreadable dump cannot confirm a receiver"
        );
        assert!(report.cell_broadcast.reason.contains("could not be read")
            || report.cell_broadcast.reason.contains("did not show"));
    }

    #[test]
    fn no_cell_broadcast_package_is_reported_as_such_not_as_unsupported_silently() {
        let report = build_report(&[], &[], Vec::new());
        assert!(!report.cell_broadcast.package_present);
        assert!(report.cell_broadcast.reason.contains("No Cell Broadcast component package"));
        assert_eq!(report.device.model, None);
    }

    // -------------------------------------------------------------------------------------------
    // Property parsing
    // -------------------------------------------------------------------------------------------

    #[test]
    fn a_getprop_batch_is_parsed_including_empty_values() {
        let text = "[ro.product.model]: [SM-A356B]\n[ro.boot.verifiedbootstate]: []\nnot a property\n";
        let parsed = parse_getprop_batch(text);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0], ("ro.product.model".to_string(), "SM-A356B".to_string()));
        assert_eq!(parsed[1].1, "", "an empty property is read, not dropped");
    }

    #[test]
    fn an_unset_property_is_none_rather_than_an_empty_string() {
        let properties = vec![("ro.product.model".to_string(), "SM-A356B".to_string())];
        let device = DeviceSnapshot::from_properties(&properties);
        assert_eq!(device.model.as_deref(), Some("SM-A356B"));
        assert_eq!(device.debuggable, None, "an unread property must stay None");
        assert_eq!(device.verified_boot_state, None);
    }

    // -------------------------------------------------------------------------------------------
    // Report rendering
    // -------------------------------------------------------------------------------------------

    #[test]
    fn the_rendered_report_leads_with_what_is_known_and_unknown() {
        let properties = vec![
            ("ro.debuggable".to_string(), "0".to_string()),
            ("ro.build.type".to_string(), "user".to_string()),
            ("ro.product.model".to_string(), "SM-A356B".to_string()),
        ];
        let commands = vec![CommandResult {
            label: "device properties".to_string(),
            command: "getprop".to_string(),
            exit_code: Some(0),
            stdout: "[ro.product.model]: [SM-A356B]".to_string(),
            stderr: String::new(),
            parsed: "1 properties parsed".to_string(),
        }];
        let report = build_report(&properties, &[cb_package_with_receiver()], commands);
        let text = render_report(&report);

        let known = text.find("## What we know").expect("known section");
        let verified = text.find("## What we verified").expect("verified section");
        let unverified = text.find("## What we could not verify").expect("unverified section");
        let commands_at = text.find("## Commands run").expect("commands section");
        assert!(known < verified && verified < unverified && unverified < commands_at);

        assert!(text.contains("SM-A356B"));
        assert!(text.contains("ro.debuggable=0"), "the blocking reason must be in the report");
        assert!(
            text.contains("an experiment"),
            "the report must not present the trial as done"
        );
        // The raw command output must be present so the reader can re-check the claim.
        assert!(text.contains("[ro.product.model]: [SM-A356B]"));
    }

    /// The report must carry the whole enumerated entry-point surface, not just the one blocked
    /// broadcast. A reader who sees only `SMS_CB_RECEIVED` would reasonably ask whether anything
    /// else was tried, and this section is the answer.
    #[test]
    fn the_report_enumerates_the_native_test_surface() {
        let properties = vec![
            ("ro.product.model".to_string(), "SM-A356B".to_string()),
            ("ro.debuggable".to_string(), "0".to_string()),
        ];
        let report = build_report(&properties, &[cb_package_with_receiver()], Vec::new());
        let text = render_report(&report);

        assert!(text.contains("## The native test surface, enumerated"));
        assert!(text.contains(crate::platform::TEST_TRIGGER_ACTION));
        assert!(text.contains("SECRET_CODE"));
        assert!(text.contains("PROTECTED BROADCAST"));
        assert!(text.contains("REACHABLE"));
        // The enumeration is a fact about Android, so it sits in the verified section.
        let verified = text.find("## What we verified").expect("verified section");
        let surface = text.find("## The native test surface").expect("surface section");
        let unverified = text.find("## What we could not verify").expect("unverified section");
        assert!(verified < surface && surface < unverified);
    }

    #[test]
    fn a_property_that_was_not_read_says_so_rather_than_showing_blank() {
        let report = build_report(&[], &[], Vec::new());
        let text = render_report(&report);
        assert!(text.contains("not read"));
        assert!(!text.contains("| Model |  |"));
    }
}
