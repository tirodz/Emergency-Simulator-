//! Platform-level facts about an Android device, derived from raw command output.
//!
//! Everything here is a pure function over text a device produced. The I/O lives in `lib.rs`;
//! this module only decides what the text means. That split is deliberate: the capability
//! questions this project keeps getting wrong (is the permission granted? does the test entry
//! point exist? what is this package actually for?) are parsing questions, and parsing is
//! testable against captured device output in a way that a `Command` invocation is not.

use serde::Serialize;

/// AOSP test-injection action.
///
/// Verified against `GsmInboundSmsHandler` in `frameworks/opt/telephony`: the receiver is
/// registered only when `ro.debuggable == 1`, and it is the supported way to place a synthetic
/// Cell Broadcast through the real telephony pipeline without a radio.
pub const TEST_TRIGGER_ACTION: &str =
    "com.android.internal.telephony.gsm.TEST_TRIGGER_CELL_BROADCAST";

/// `SmsCbConstants.MESSAGE_ID_ETWS_TEST_MESSAGE` (3GPP TS 23.041 message identifier 0x1103).
pub const MESSAGE_ID_ETWS_TEST: u16 = 0x1103;

/// `SmsCbConstants.MESSAGE_ID_ETWS_EARTHQUAKE_WARNING` (0x1100).
pub const MESSAGE_ID_ETWS_EARTHQUAKE: u16 = 0x1100;

/// Fixes the ETWS test message as the only class this tool can emit.
pub const ETWS_TEST_SERVICE_CATEGORY: u32 = 4355;

/// The property whose value decides whether the AOSP test receiver exists at all.
pub const DEBUGGABLE_PROPERTY: &str = "ro.debuggable";

/// Discover runtime-registered test receiver actions from ActivityManager's live receiver table.
///
/// Dynamic receivers do not appear in the static package resolver table. The AOSP GSM test
/// receiver is a runtime registration, so this second discovery surface can catch an OEM/vendor
/// test receiver that is active on a retail build.
pub fn discover_runtime_test_actions(dumpsys: &str) -> Vec<String> {
    let mut receiver_relevant = false;
    let mut out = Vec::new();

    for raw in dumpsys.lines() {
        let line = raw.trim();

        if line.starts_with('*') && line.contains("ReceiverList{") {
            receiver_relevant = false;
            continue;
        }

        if line.starts_with("app=") {
            let lower = line.to_ascii_lowercase();
            receiver_relevant = ["cellbroadcast", "telephony", "samsung"]
                .iter()
                .any(|token| lower.contains(token));
            continue;
        }

        if !receiver_relevant || !line.starts_with("Action:") {
            continue;
        }

        let action = line
            .split_once('"')
            .and_then(|(_, rest)| rest.split_once('"').map(|(value, _)| value));

        let Some(action) = action else { continue };
        let upper = action.to_ascii_uppercase();
        let eligible = upper.contains("TEST")
            && ["CELL", "BROADCAST", "EMERGENCY", "ALERT", "CMAS", "ETWS"]
                .iter()
                .any(|token| upper.contains(token));

        if eligible && !out.iter().any(|value| value == action) {
            out.push(action.to_string());
        }
    }

    out.truncate(12);
    out
}

/// Discover exported diagnostic receiver actions from package-manager output.
/// Only actions explicitly named as TEST plus a broadcast/emergency token are returned.
pub fn discover_test_actions(dumpsys: &str) -> Vec<String> {
    let mut in_receiver = false;
    let mut exported = false;
    let mut relevant_package = false;
    let mut out = Vec::new();

    for raw in dumpsys.lines() {
        let line = raw.trim();
        if line.starts_with("Package [") {
            let p = line.to_ascii_lowercase();
            relevant_package = ["cellbroadcast", "telephony", "samsung"]
                .iter()
                .any(|token| p.contains(token));
            in_receiver = false;
            exported = false;
        } else if line.contains("Receiver{") {
            in_receiver = relevant_package;
            exported = line.contains("exported=true");
        } else if !in_receiver {
            continue;
        } else if line.contains("exported=true") {
            exported = true;
        } else if line.starts_with("Action:") || line.starts_with("action=") {
            let action = line
                .split_once('"')
                .and_then(|(_, r)| r.split_once('"').map(|(a, _)| a))
                .or_else(|| line.split_once('=').map(|(_, a)| a.trim_matches('"').trim()));
            if let Some(action) = action {
                let u = action.to_ascii_uppercase();
                let eligible = exported
                    && u.contains("TEST")
                    && ["CELL", "BROADCAST", "EMERGENCY", "ALERT", "CMAS", "ETWS"]
                        .iter()
                        .any(|token| u.contains(token));
                if eligible && !out.iter().any(|x| x == action) {
                    out.push(action.to_string());
                }
            }
        }
    }

    out.truncate(12);
    out
}
    #[test]
    fn discovers_runtime_registered_cellbroadcast_test_action() {
        let dump = r#"
          * ReceiverList{123 456 com.android.internal.telephony/1001/u0 remote:789}
            app=456:com.android.internal.telephony/u0a123 pid=456 uid=1001 user=0
            Filter #0: BroadcastFilter{abc}
              Action: "com.android.internal.telephony.gsm.TEST_TRIGGER_CELL_BROADCAST"
        "#;
        assert_eq!(
            discover_runtime_test_actions(dump),
            vec!["com.android.internal.telephony.gsm.TEST_TRIGGER_CELL_BROADCAST".to_string()]
        );
    }


/// One alert channel this tool can address, together with the receive-side gate that decides
/// whether `/packages/apps/CellBroadcastReceiver` will actually raise it.
///
/// The catalogue exists because the channel is not a cosmetic choice. AOSP routes a received
/// message through `CellBroadcastAlertService.isChannelEnabled`, which looks the channel up in the
/// range arrays in `res/values/config.xml` and then consults a *different* user preference per
/// array. Two messages with identical text and identical encoding can therefore behave in opposite
/// ways on the same phone, and the one this project used to hardcode is the worst of them:
///
/// | Channel | Preference `isChannelEnabled` consults | AOSP default |
/// |---|---|---|
/// | 0x1100 ETWS primary | `KEY_ENABLE_ALERTS_MASTER_TOGGLE` only | **on** |
/// | 0x1113 CMAS extreme | master **and** `KEY_ENABLE_CMAS_EXTREME_THREAT_ALERTS` | **on** |
/// | 0x1103 ETWS test | master **and** test-alerts toggle **and** test-mode | **off** |
/// | 0x111C monthly test | master **and** test-alerts toggle | **off** |
///
/// `0x1103` is enabled *only* while the device is in testing mode, which on a retail build is the
/// `allow_testing_mode_on_user_build` default plus the operator dialling the `2627` secret code.
/// A channel that is off by default turns an injection that worked perfectly into silence, which is
/// indistinguishable from an injection that never ran — the exact confusion this project exists to
/// remove. The catalogue makes that choice explicit and testable instead of a buried constant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct AlertChannel {
    /// 3GPP TS 23.041 message identifier, which AOSP also calls the service category.
    pub message_id: u16,
    /// Human label, including the identifier so logs can be matched against this table.
    pub label: &'static str,
    /// A one-line statement of what the user sees if this is delivered.
    pub effect: &'static str,
    /// Whether the receive-side default is on.
    pub enabled_by_default: bool,
    /// The preference name `isChannelEnabled` consults, beyond the master toggle.
    pub gate: &'static str,
    /// What the operator must change, if anything, for this channel to be delivered.
    pub requirement: &'static str,
}

/// The channels worth addressing, ordered best-first by how little they need from the operator.
///
/// The ordering is the point: the first entry needs nothing switched on, so it is the one to try
/// before asking anyone to change a setting. The list is deliberately short — these are the
/// channels whose gating was read out of the AOSP source in `isChannelEnabled`, not a dump of every
/// identifier in `SmsCbConstants`.
pub const ALERT_CHANNELS: &[AlertChannel] = &[
    AlertChannel {
        message_id: MESSAGE_ID_ETWS_EARTHQUAKE,
        label: "ETWS EARTHQUAKE WARNING (0x1100)",
        effect: "Full-screen alert with the emergency tone and vibration; ETWS alerts cannot be \
                 opted out of individually.",
        enabled_by_default: true,
        gate: "master toggle only",
        requirement: "Nothing. Enabled on a default device.",
    },
    AlertChannel {
        message_id: 0x1101,
        label: "ETWS TSUNAMI WARNING (0x1101)",
        effect: "As 0x1100.",
        enabled_by_default: true,
        gate: "master toggle only",
        requirement: "Nothing. Enabled on a default device.",
    },
    AlertChannel {
        message_id: 0x1113,
        label: "CMAS EXTREME THREAT (0x1113)",
        effect: "Highest-priority CMAS alert; full-screen and loud.",
        enabled_by_default: true,
        gate: "KEY_ENABLE_CMAS_EXTREME_THREAT_ALERTS",
        requirement: "Nothing by default; must not be switched off in Emergency alerts settings.",
    },
    AlertChannel {
        message_id: 0x111C,
        label: "CMAS REQUIRED MONTHLY TEST (0x111C)",
        effect: "A genuine test alert: the real emergency tone, full-screen, and the text prefixed \
                 as a test message.",
        enabled_by_default: false,
        gate: "KEY_ENABLE_TEST_ALERTS",
        requirement: "Turn on the test-alerts toggle (Emergency alerts settings, or the 2627 code).",
    },
    AlertChannel {
        message_id: MESSAGE_ID_ETWS_TEST,
        label: "ETWS TEST MESSAGE (0x1103)",
        effect: "The AOSP test alert, and the channel AOSP's own test receiver is built around.",
        enabled_by_default: false,
        gate: "KEY_ENABLE_TEST_ALERTS and testing mode",
        requirement: "Enable testing mode (2627) AND the test-alerts toggle.",
    },
];

/// Look up a channel by message identifier.
pub fn alert_channel(message_id: u16) -> Option<&'static AlertChannel> {
    ALERT_CHANNELS.iter().find(|c| c.message_id == message_id)
}

/// The channel to try first: the one that needs nothing changed on a default device.
pub fn default_alert_channel() -> &'static AlertChannel {
    &ALERT_CHANNELS[0]
}

const POST_NOTIFICATIONS: &str = "android.permission.POST_NOTIFICATIONS";

/// The result of asking "is this thing true?" about a device.
///
/// The distinction that matters throughout this project is `Denied` versus `Unknown`. Collapsing
/// `Unknown` into `Denied` produces a confident, wrong instruction to the operator ("grant
/// notifications, then retry") when the real problem is that nothing could be parsed. A
/// capability probe that lies is worse than one that admits ignorance.
///
/// `Default` is `Unknown` on purpose: a capability nobody has checked must never start life
/// looking like a denial.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum State {
    Granted,
    Denied,
    NotPresent,
    /// The question does not apply to this device or this mode. Distinct from `NotPresent`,
    /// which means the thing was looked for and is absent.
    NotApplicable,
    Unknown,
    Error,
}

impl Default for State {
    fn default() -> Self {
        State::Unknown
    }
}

impl State {
    pub fn label(self) -> &'static str {
        match self {
            State::Granted => "GRANTED",
            State::Denied => "DENIED",
            State::NotPresent => "NOT_PRESENT",
            State::NotApplicable => "NOT_APPLICABLE",
            State::Unknown => "UNKNOWN",
            State::Error => "ERROR",
        }
    }

    /// True only for an affirmative answer. Callers must not treat `Unknown` as `Denied`.
    pub fn is_granted(self) -> bool {
        self == State::Granted
    }

    /// True when the answer is a fact about the device rather than a gap in our knowledge.
    ///
    /// `Unknown` and `Error` are the two states a probe produces when it *failed to find out*
    /// something. Everywhere else in this module they are kept distinct from a real negative
    /// result; this predicate exists so a report can say "we could not determine this" without
    /// enumerating both states at each site.
    pub fn is_definite(self) -> bool {
        !matches!(self, State::Unknown | State::Error)
    }
}

/// How far along the chain to a genuine platform test broadcast a device actually is.
///
/// These are ordered, and each one is a strictly stronger claim than the last. The old UI jumped
/// from "a package with 'cellbroadcast' in its name exists" to "Cell Broadcast is supported",
/// which skips every state that matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CapabilityStage {
    /// No Cell Broadcast component can be found at all.
    None,
    /// A package matching Cell Broadcast exists. Proves almost nothing on its own.
    PackagePresent,
    /// The package declares the Cell Broadcast receiver/service this project needs.
    ReceiverDiscovered,
    /// The device's build permits the AOSP test entry point (`ro.debuggable=1`).
    TestEntryPointDiscovered,
    /// A test broadcast was accepted and the receiver ran.
    TestEntryPointAccepted,
    /// Downstream evidence shows the Cell Broadcast service processed the message.
    CellBroadcastServiceReached,
    /// The Cell Broadcast receiver ran for a Cell-Broadcast action.
    ///
    /// This sits above the service stage only because the receiver re-dispatches into the alert
    /// service; it is still *before* the alert service has looked at the message. It is deliberately
    /// **not** `SystemUiReached`: the receiver running is not the UI being presented, and the old
    /// mapping jumped that gap.
    ReceiverProcessed,
    /// The alert service started on the Cell-Broadcast action.
    ///
    /// This is the point the message enters `CellBroadcastAlertService`. It is still *not* delivery:
    /// the service's channel-range, testing-mode and language checks run after it starts, which is
    /// exactly why the suppression markers are consulted before this stage is reported as success.
    AlertServiceReached,
    /// Downstream evidence shows Android's own alert UI was requested.
    SystemUiReached,
}

impl CapabilityStage {
    pub fn label(self) -> &'static str {
        match self {
            CapabilityStage::None => "NO CELL BROADCAST COMPONENT",
            CapabilityStage::PackagePresent => "PACKAGE PRESENT",
            CapabilityStage::ReceiverDiscovered => "RECEIVER DISCOVERED",
            CapabilityStage::TestEntryPointDiscovered => "TEST ENTRY POINT DISCOVERED",
            CapabilityStage::TestEntryPointAccepted => "TEST ENTRY POINT ACCEPTED",
            CapabilityStage::CellBroadcastServiceReached => "CB SERVICE REACHED",
            CapabilityStage::ReceiverProcessed => "CB RECEIVER PROCESSED",
            CapabilityStage::AlertServiceReached => "ALERT SERVICE REACHED",
            CapabilityStage::SystemUiReached => "NATIVE ALERT PRESENTED",
        }
    }
}

/// The ordered stages a single send is judged against, named for the verification ladder.
///
/// This is a separate, finer-grained ladder than [`CapabilityStage`]: [`CapabilityStage`] answers
/// "what can this device do in general", and this answers "what did this one attempt actually
/// achieve". The distinction matters for the honesty rule -- a run stops at the first stage it
/// cannot prove and is reported as `FAILED` or `UNKNOWN` there, never as `SUCCESS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VerificationStage {
    DeviceConnected,
    DeviceIdentified,
    CapabilitiesDetected,
    NativePathSelected,
    LoggingArmed,
    TriggerSent,
    TelephonyActivityDetected,
    CellBroadcastProcessingDetected,
    AlertServiceDetected,
    NativeAlertDetected,
    Success,
}

/// One rung of the ladder, with the log evidence that would prove it.
pub struct VerificationRung {
    pub stage: VerificationStage,
    pub label: &'static str,
    /// The evidence that must be present for this rung to count as reached.
    pub requires: &'static str,
}

/// The full ladder, in order. A caller walks it and stops at the first rung it cannot evidence.
pub fn verification_ladder() -> Vec<VerificationRung> {
    use VerificationStage::*;
    vec![
        VerificationRung {
            stage: DeviceConnected,
            label: "DEVICE_CONNECTED",
            requires: "adb reports the device as `device`",
        },
        VerificationRung {
            stage: DeviceIdentified,
            label: "DEVICE_IDENTIFIED",
            requires: "model, build fingerprint and ro.debuggable were read",
        },
        VerificationRung {
            stage: CapabilitiesDetected,
            label: "CAPABILITIES_DETECTED",
            requires: "a Cell Broadcast receiver package was found and its manifest read",
        },
        VerificationRung {
            stage: NativePathSelected,
            label: "NATIVE_PATH_SELECTED",
            requires: "the native test path or a named native alert action was chosen for this build",
        },
        VerificationRung {
            stage: LoggingArmed,
            label: "LOGGING_ARMED",
            requires: "logcat was cleared immediately before the trigger",
        },
        VerificationRung {
            stage: TriggerSent,
            label: "TRIGGER_SENT",
            requires: "the trigger left the desktop and the transport did not fail",
        },
        VerificationRung {
            stage: TelephonyActivityDetected,
            label: "TELEPHONY_ACTIVITY_DETECTED",
            requires: "the telephony test receiver logged receiving the intent",
        },
        VerificationRung {
            stage: CellBroadcastProcessingDetected,
            label: "CELL_BROADCAST_PROCESSING_DETECTED",
            requires: "the Cell Broadcast service/handler logged processing the message",
        },
        VerificationRung {
            stage: AlertServiceDetected,
            label: "ALERT_SERVICE_DETECTED",
            requires: "CellBroadcastAlertService logged starting on the Cell-Broadcast action",
        },
        VerificationRung {
            stage: NativeAlertDetected,
            label: "NATIVE_ALERT_DETECTED",
            requires: "the platform logged selecting the native alert presentation",
        },
        VerificationRung {
            stage: Success,
            label: "SUCCESS",
            requires: "NATIVE_ALERT_DETECTED with no suppression marker in the capture",
        },
    ]
}

/// The alert paths this controller can take.
///
/// These are kept as distinct states on purpose. The defect this project exists to prevent is a
/// local app notification being presented as Cell Broadcast delivery, so "the local simulator is
/// installed" and "Android's Cell Broadcast pipeline is reachable" must never collapse into one
/// notion of "ready".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AlertMode {
    /// The bundled local simulator app posts a notification on the phone: sound, vibration,
    /// full-screen UI. It is a local app alert and **not** Android's Cell Broadcast stack, so the
    /// UI must label it as a simulation.
    LocalUiSimulation,
    /// The AOSP telephony test entry point. The message is decoded by the telephony process into a
    /// real `SmsCbMessage` and enters Android's Cell Broadcast pipeline.
    PlatformCellBroadcastTest,
    /// Android's own alert UI was observed downstream after a platform send. This is the only mode
    /// that represents a genuine alert, and it is established per-send from logcat evidence — never
    /// inferred from the device or from a command's exit code.
    GenuineCellBroadcastVerified,
    /// No path is available on this build: the test entry point is not present and no local
    /// simulator is installed. A valid, reportable outcome.
    Unavailable,
}

impl AlertMode {
    pub fn label(self) -> &'static str {
        match self {
            AlertMode::LocalUiSimulation => "LOCAL UI SIMULATION",
            AlertMode::PlatformCellBroadcastTest => "PLATFORM CELL BROADCAST TEST",
            AlertMode::GenuineCellBroadcastVerified => "GENUINE CELL BROADCAST — VERIFIED",
            AlertMode::Unavailable => "UNAVAILABLE",
        }
    }

    /// True only for the two modes that involve Android's Cell Broadcast stack. The UI uses this to
    /// decide whether it may use Cell Broadcast wording at all.
    pub fn is_cell_broadcast(self) -> bool {
        matches!(
            self,
            AlertMode::PlatformCellBroadcastTest | AlertMode::GenuineCellBroadcastVerified
        )
    }
}

/// Choose the mode from what the device actually reports.
///
/// The ordering encodes one decision: the platform path is preferred whenever it exists, because it
/// is the only one that reaches Android's Cell Broadcast stack. The local simulator is a fallback
/// for testing the alert *presentation*, never a substitute for delivery.
///
/// `verified` is passed in rather than derived, because it can only come from downstream logcat
/// evidence for a specific send. A caller with no evidence must pass `false`, and then this function
/// cannot return the verified mode — an assertion that no code path can claim verification for free.
pub fn alert_mode(
    entrypoint: State,
    local_simulator_installed: bool,
    verified: bool,
) -> AlertMode {
    if verified {
        return AlertMode::GenuineCellBroadcastVerified;
    }
    if entrypoint == State::Granted {
        return AlertMode::PlatformCellBroadcastTest;
    }
    if local_simulator_installed {
        return AlertMode::LocalUiSimulation;
    }
    AlertMode::Unavailable
}

/// The strongest capability stage established by evidence.
///
/// This is the single place that decides the stage, so the devices list and the diagnostics panel
/// cannot disagree. The previous version promoted a device to `TestEntryPointDiscovered` when the
/// **local simulator** was installed, which reported a platform Cell Broadcast capability on the
/// strength of an unrelated local app. That is the conflation this project forbids.
pub fn capability_stage(
    receiver_declared: bool,
    package_present: bool,
    entrypoint: State,
) -> CapabilityStage {
    if receiver_declared && entrypoint == State::Granted {
        CapabilityStage::TestEntryPointDiscovered
    } else if receiver_declared {
        CapabilityStage::ReceiverDiscovered
    } else if package_present {
        CapabilityStage::PackagePresent
    } else {
        CapabilityStage::None
    }
}

/// Whether the AOSP test receiver will be present on this build.
///
/// `GsmInboundSmsHandler` gates registration on `ro.debuggable`, so this is a hard boundary rather
/// than a permission that can be granted. Retail firmware ships `ro.debuggable=0` and the receiver
/// is never constructed, which means the broadcast is accepted by `am` and then silently does
/// nothing. The controller must say that in advance instead of reporting a delivery that never
/// happened.
pub fn debuggable_permits_test_entrypoint(value: &str) -> Option<bool> {
    match value.trim() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

/// Parse `granted=<bool>` out of a permission entry.
///
/// Returns `None` when the text carries no explicit grant state, which is the common case for the
/// `requested permissions:` section: there the permission is named with no `granted=` at all.
/// Treating that as "not granted" is precisely the bug this function exists to prevent.
pub fn parse_granted_flag(text: &str) -> Option<State> {
    let index = text.find("granted=")?;
    let value: String = text[index + "granted=".len()..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    match value.as_str() {
        "true" => Some(State::Granted),
        "false" => Some(State::Denied),
        _ => None,
    }
}

/// Decide the runtime state of `POST_NOTIFICATIONS` from `dumpsys package <pkg>`.
///
/// Deliberately line-oriented rather than "first line containing the permission", which is what
/// made the original probe wrong. AOSP and Samsung both print the permission several times: in
/// `requested permissions:`, in `install permissions:`, and in `runtime permissions:` with the
/// actual grant state. Only the last carries `granted=`, and a bare mention is not a denial.
pub fn parse_post_notifications(dump: &str) -> State {
    if dump.trim().is_empty() {
        return State::Unknown;
    }
    let lower = dump.to_ascii_lowercase();
    if lower.contains("unable to find package") || lower.contains("unknown package") {
        return State::NotPresent;
    }

    let mut mentioned = false;
    for line in dump.lines() {
        let Some(index) = line.find(POST_NOTIFICATIONS) else {
            continue;
        };
        mentioned = true;
        let rest = &line[index + POST_NOTIFICATIONS.len()..];
        if let Some(state) = parse_granted_flag(rest) {
            return state;
        }
    }

    // The permission appears only in a section that does not carry a grant state. That is an
    // absence of evidence, not evidence of denial.
    if mentioned {
        State::Unknown
    } else {
        State::NotPresent
    }
}

/// Decide the state of an app-op from `appops get <pkg> <OP>`.
///
/// `allowed`/`deny` are the two decisive words. Anything else (an absent op, `default`,
/// `ignore`, or unexpected OEM formatting) is reported as `Unknown` rather than folded into a
/// denial, and `default` specifically is not a denial: for `USE_FULL_SCREEN_INTENT` on Android 14+
/// the default *is* denial, but for other ops it is not, so the caller decides.
pub fn parse_appop(text: &str) -> State {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return State::Unknown;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("no such package") || lower.contains("unknown package") {
        return State::NotPresent;
    }
    if lower.contains("unknown operation") || lower.contains("no operation") {
        return State::NotPresent;
    }
    for line in lower.lines() {
        let Some((_, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if value == "allow" || value == "allowed" {
            return State::Granted;
        }
        if value.starts_with("deny") || value.starts_with("ignore") {
            return State::Denied;
        }
    }
    State::Unknown
}

/// Decide whether Android will present a full-screen intent for this app.
///
/// On Android 14+ `USE_FULL_SCREEN_INTENT` defaults to denied for apps that are not calling or
/// alarm apps, and a denied op degrades an alert into a heads-up notification without any error.
/// Unlike [`parse_appop`], `default`/absent is treated as denied here because that is the platform
/// behaviour for this specific op.
pub fn parse_full_screen_intent(text: &str) -> State {
    let lower = text.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return State::Unknown;
    }
    if lower.contains("unknown operation") || lower.contains("no such package") {
        return State::NotPresent;
    }
    if lower.contains("allow") {
        return State::Granted;
    }
    if lower.contains("deny") || lower.contains("ignore") {
        return State::Denied;
    }
    if lower.contains("default") {
        return State::Denied;
    }
    State::Unknown
}

/// Extract `key=value` pairs from any whitespace-separated property dump.
pub fn parse_property(text: &str, key: &str) -> Option<String> {
    let prefix = format!("[{key}]:");
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(&prefix) {
            let value = rest.trim().trim_start_matches('[').trim_end_matches(']').trim();
            return Some(value.to_string());
        }
    }
    None
}

/// Pick the package that is actually providing Cell Broadcast alerting.
///
/// Name matching alone is not evidence, so the caller must pair this with a check that the chosen
/// package really declares the receiver. Ranking exists only to decide order between genuine
/// candidates, and prefers the dedicated receiver module over a service module.
pub fn rank_cellbroadcast_packages(packages: &[String]) -> Vec<String> {
    let mut ranked: Vec<String> = packages
        .iter()
        .filter(|package| package.to_ascii_lowercase().contains("cellbroadcast"))
        .cloned()
        .collect();
    ranked.sort_by_key(|package| {
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
    ranked.dedup();
    ranked
}

// ---------------------------------------------------------------------------------------------
// The enumerated native test surface
// ---------------------------------------------------------------------------------------------

/// Why a candidate entry point into the phone's Cell Broadcast machinery does or does not work.
///
/// The point of enumerating candidates instead of reporting one verdict is that "no root-free path
/// exists" is only a useful answer if every candidate was tried and each failed for its own stated
/// reason. "SMS_CB_RECEIVED is protected" is one reason, not the answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EntryPointOutcome {
    /// A non-system caller may send it and it feeds the real pipeline. Gated only by `ro.debuggable`.
    Reachable,
    /// The action is in the platform's `<protected-broadcast>` list, so `ActivityManagerService`
    /// refuses any caller that is not a system UID. No permission can be granted to fix this.
    ProtectedBroadcast,
    /// The component is not exported, so a shell caller cannot address it even if the action is
    /// unprotected.
    NotExported,
    /// The component is reachable but only reads or changes state; it cannot originate a message.
    NotAnInjector,
    /// A signature-level permission or a build property stands in the way.
    SignatureOrBuildGate,
}

impl EntryPointOutcome {
    /// A short label for tables.
    pub fn label(self) -> &'static str {
        match self {
            EntryPointOutcome::Reachable => "REACHABLE",
            EntryPointOutcome::ProtectedBroadcast => "PROTECTED BROADCAST",
            EntryPointOutcome::NotExported => "NOT EXPORTED",
            EntryPointOutcome::NotAnInjector => "NOT AN INJECTOR",
            EntryPointOutcome::SignatureOrBuildGate => "SIGNATURE/BUILD GATE",
        }
    }

    /// One line an operator can act on.
    pub fn explain(self) -> &'static str {
        match self {
            EntryPointOutcome::Reachable => {
                "reachable by adb shell; the only remaining gate is a build property"
            }
            EntryPointOutcome::ProtectedBroadcast => {
                "a protected broadcast: the platform refuses any non-system caller before the \
                 receiver is resolved, so the permissionless `exported` flag never applies"
            }
            EntryPointOutcome::NotExported => {
                "the component is not exported, so a shell caller cannot address it"
            }
            EntryPointOutcome::NotAnInjector => {
                "reachable, but it reads or filters state and cannot originate a message"
            }
            EntryPointOutcome::SignatureOrBuildGate => {
                "behind a signature permission or a build property, neither of which can be granted"
            }
        }
    }
}

/// One way the phone's Cell Broadcast machinery might be driven, and what stands in the way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntryPoint {
    pub component: &'static str,
    pub action: &'static str,
    pub outcome: EntryPointOutcome,
}

/// The AOSP Cell Broadcast entry-point surface, enumerated from source.
///
/// Every row was read out of `android14-release` this session; the three `Reachable` rows are the
/// only non-protected alert-bearing actions in the stack, and each is a *dynamically registered*
/// receiver gated on `ro.debuggable`, which is why none has a manifest component name.
///
/// This list is the answer to "did we check everything?". It is deliberately exhaustively boring:
/// the value is in the rows that look promising and are not.
pub fn entry_points() -> Vec<EntryPoint> {
    let row = |component, action, outcome| EntryPoint {
        component,
        action,
        outcome,
    };
    vec![
        row(
            "CellBroadcastReceiver",
            "android.provider.action.SMS_EMERGENCY_CB_RECEIVED",
            EntryPointOutcome::ProtectedBroadcast,
        ),
        row(
            "CellBroadcastReceiver",
            "android.provider.Telephony.SMS_CB_RECEIVED",
            EntryPointOutcome::ProtectedBroadcast,
        ),
        row(
            "CellBroadcastReceiver",
            "android.provider.Telephony.SMS_SERVICE_CATEGORY_PROGRAM_DATA_RECEIVED",
            EntryPointOutcome::ProtectedBroadcast,
        ),
        row(
            "CellBroadcastReceiver",
            "android.telephony.action.DEFAULT_SMS_SUBSCRIPTION_CHANGED",
            EntryPointOutcome::ProtectedBroadcast,
        ),
        row(
            "CellBroadcastReceiver",
            "android.telephony.action.CARRIER_CONFIG_CHANGED",
            EntryPointOutcome::ProtectedBroadcast,
        ),
        row(
            "CellBroadcastReceiver",
            "android.intent.action.SERVICE_STATE",
            EntryPointOutcome::ProtectedBroadcast,
        ),
        row(
            "CellBroadcastReceiver",
            "android.intent.action.LOCALE_CHANGED",
            EntryPointOutcome::ProtectedBroadcast,
        ),
        row(
            "CellBroadcastReceiver",
            "android.intent.action.BOOT_COMPLETED",
            EntryPointOutcome::ProtectedBroadcast,
        ),
        // The secret code is a protected broadcast *and* a gate-opener rather than an injector.
        // BUG-002's reproduction used `am broadcast` for this action, which the platform refuses;
        // the toggle-not-idempotent lesson still holds, but the reproduction did not.
        row(
            "CellBroadcastReceiver",
            "android.telephony.action.SECRET_CODE (2627 / CMAS)",
            EntryPointOutcome::ProtectedBroadcast,
        ),
        row(
            "GsmCbTestBroadcastReceiver (dynamic)",
            TEST_TRIGGER_ACTION,
            EntryPointOutcome::Reachable,
        ),
        row(
            "CdmaCbTestBroadcastReceiver (dynamic)",
            "com.android.internal.telephony.cdma.TEST_TRIGGER_CELL_BROADCAST",
            EntryPointOutcome::Reachable,
        ),
        row(
            "CdmaCbTestBroadcastReceiver (dynamic)",
            "com.android.internal.telephony.cdma.TEST_TRIGGER_SCP_MESSAGE",
            EntryPointOutcome::Reachable,
        ),
        row(
            "CellBroadcastAlertService",
            "cellbroadcastreceiver.SHOW_NEW_ALERT",
            EntryPointOutcome::NotExported,
        ),
        row(
            "CellBroadcastAlertDialog",
            "android.provider.Telephony.SMS_CB_RECEIVED",
            EntryPointOutcome::NotExported,
        ),
        row(
            "persist.cellbroadcast.message_filter",
            "(system property)",
            EntryPointOutcome::NotAnInjector,
        ),
        row(
            "DefaultCellBroadcastService",
            "android.telephony.CellBroadcastService",
            EntryPointOutcome::SignatureOrBuildGate,
        ),
    ]
}

/// Which entry points, if any, lead into the phone's own alert pipeline without a system UID.
pub fn reachable_entry_points() -> Vec<EntryPoint> {
    entry_points()
        .into_iter()
        .filter(|entry| entry.outcome == EntryPointOutcome::Reachable)
        .collect()
}

/// A sentence stating the whole surface, for the diagnostics report.
///
/// Built from [`entry_points`] so the prose cannot drift from the enumeration: if a row is added or
/// its outcome changes, this sentence changes with it.
pub fn entry_point_summary() -> String {
    let all = entry_points();
    let reachable = reachable_entry_points();
    let protected = all
        .iter()
        .filter(|entry| entry.outcome == EntryPointOutcome::ProtectedBroadcast)
        .count();
    let other = all.len() - reachable.len() - protected;
    format!(
        "{} entry points were enumerated across the Cell Broadcast stack: {} are protected \
         broadcasts (refused before the receiver is resolved), {} are behind an `exported=\"false\"` \
         component, a signature permission or a build property, and {} are reachable by adb shell. \
         The reachable ones are the telephony test receivers, registered only when ro.debuggable=1, \
         and they feed the genuine pipeline rather than a copy of it.",
        all.len(),
        protected,
        other,
        reachable.len()
    )
}

// ---------------------------------------------------------------------------------------------
// The interface classes that are NOT a broadcast
// ---------------------------------------------------------------------------------------------
//
// Everything above enumerates *broadcast* candidates. That enumeration is complete and it is also
// not the whole answer, because the alert pipeline has a second kind of entry: a Binder call into
// the Cell Broadcast service module. The previous pass stopped at the broadcast layer and therefore
// reported the reachable set as "the telephony test receivers", which is true only of broadcasts.
//
// This section walks the classes the broadcast matrix does not cover -- shell commands, Binder
// transactions, system_server services, content-provider calls, exported OEM components -- and
// records each one's verdict with the reason. It is deliberately an enumeration rather than a
// conclusion: the point is that each interface fails, or does not fail, for its own stated reason.

/// What kind of interface a candidate is, so a reader can see the classes were covered exhaustively
/// rather than only the ones that came to mind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InterfaceClass {
    /// A `cmd <service> <verb>` shell command, i.e. a `ShellCommand` subclass on a system service.
    ShellCommand,
    /// An AIDL/Binder method on a system service, reachable by any process that can `getService`.
    BinderService,
    /// A `content://` provider `call()` or a write into a provider.
    ContentProvider,
    /// A Binder method on the platform's `ITelephony` (the `phone` service).
    TelephonyBinder,
    /// An OEM (Samsung) component: activity, service, receiver or provider.
    OemComponent,
    /// A property, dialer code or preference that only changes state.
    StateOnly,
}

/// Why an interface cannot drive the alert pipeline. The variants are the *actual* mechanisms found
/// in the A35 firmware and AOSP, not a generic taxonomy.
///
/// There is deliberately no `Reachable` variant: every non-broadcast interface class was walked and
/// none of them can carry a message into the pipeline. Adding one back would require a row that
/// proves it, and the survey's own test asserts the gated set is exactly the ICellBroadcastService
/// handlers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InterfaceOutcome {
    /// The method exists but is permission-gated to a signature/system UID that adb shell does not
    /// hold, and no permission can be granted to shell to fix it.
    SignatureGate,
    /// Reachable, but its input can only configure, filter or read. It cannot construct a message.
    ConfigOnly,
    /// The interface does not exist in this build at all.
    Absent,
    /// The method *does* construct an `SmsCbMessage` and *does* feed the real alert service, and the
    /// only thing standing in the way is a build property rather than a permission.
    BuildPropertyGate,
}

impl InterfaceOutcome {
    pub fn label(self) -> &'static str {
        match self {
            InterfaceOutcome::SignatureGate => "SIGNATURE GATE",
            InterfaceOutcome::ConfigOnly => "CONFIG ONLY",
            InterfaceOutcome::Absent => "ABSENT",
            InterfaceOutcome::BuildPropertyGate => "BUILD-PROPERTY GATE",
        }
    }
}

/// One non-broadcast interface into (or beside) the Cell Broadcast machinery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InterfaceCandidate {
    pub class: InterfaceClass,
    /// The concrete interface: a command verb, a Binder method, a component name.
    pub interface: &'static str,
    pub outcome: InterfaceOutcome,
    /// The single most important fact about it, in one line.
    pub note: &'static str,
}

/// Every non-broadcast interface class that could plausibly reach the alert pipeline, and its
/// verdict. Ordered by class so the enumeration reads as a survey.
///
/// The reason this exists as data rather than prose is the same reason [`entry_points`] does: the
/// summary is generated from it, so a claim cannot drift away from the row that supports it.
pub fn interface_candidates() -> Vec<InterfaceCandidate> {
    let row = |class, interface, outcome, note| InterfaceCandidate {
        class,
        interface,
        outcome,
        note,
    };
    vec![
        // -- The shell-command layer ---------------------------------------------------------
        row(
            InterfaceClass::ShellCommand,
            "cmd phone  (TelephonyShellCommand verbs)",
            InterfaceOutcome::ConfigOnly,
            "The full verb list was read out of TeleService.apk: ims, uce, cc, gba, src, d2d, \
             data, radio, euicc, barring, emergency-number-test-mode, emergency-callback-mode, \
             thermal-mitigation, restart-modem, unattended-reboot, get-imei, numverify. Not one \
             verb constructs an SmsCbMessage or calls the alert service; they configure or query.",
        ),
        row(
            InterfaceClass::ShellCommand,
            "cmd phone radio set-modem-service <name>",
            InterfaceOutcome::Absent,
            "The only verb that could replay radio events, including broadcast SMS, through the \
             real mCi path -- MockModemService replays RIL events. It is unusable here: selecting \
             it calls ITelephony.setModemService, which is enforced on the signature permission \
             android.permission.MODIFY_PHONE_STATE, and the com.android.telephony.mockmodem \
             package that would implement the service is not present in the A35 image.",
        ),
        row(
            InterfaceClass::ShellCommand,
            "cmd phone carrier_restriction_status_test",
            InterfaceOutcome::ConfigOnly,
            "Present, but gated on the MockModem service being the active modem service, and it \
             only rewrites the carrier allow-list JSON. Not an injector.",
        ),
        row(
            InterfaceClass::ShellCommand,
            "cmd cellbroadcast  (any verb)",
            InterfaceOutcome::Absent,
            "The Cell Broadcast service module implements no ShellCommand at all: \
             DefaultCellBroadcastService has onGsmCellBroadcastSms / onCdmaCellBroadcastSms / \
             onCdmaScpMessage / getCellBroadcastAreaInfo and a dump(), and nothing else. There is \
             no command surface to find.",
        ),
        row(
            InterfaceClass::ShellCommand,
            "cmd activity / cmd package / cmd carrier_config",
            InterfaceOutcome::ConfigOnly,
            "Surveyed for test or injection verbs. carrier_config sets and reads carrier \
             configuration; activity and package manipulate components. None produces telephony \
             data.",
        ),
        // -- Binder services ------------------------------------------------------------------
        row(
            InterfaceClass::BinderService,
            "android.telephony.ICellBroadcastService.handleGsmCellBroadcastSms(phoneId, byte[])",
            InterfaceOutcome::BuildPropertyGate,
            "This is the real thing: the method the framework calls with a raw GSM broadcast PDU, \
             which decodes it into an SmsCbMessage and hands it to the genuine alert service. \
             Two routes reach it. The first is the telephony test receiver, registered only when \
             ro.debuggable=1. The second is mCi.setOnNewGsmBroadcastSms, the radio callback, and \
             adb shell has no way to raise a radio event without the MockModem service above. \
             Binding the service directly needs the signature permission \
             android.permission.BIND_CELL_BROADCAST_SERVICE.",
        ),
        row(
            InterfaceClass::BinderService,
            "android.telephony.ICellBroadcastService.handleCdmaCellBroadcastSms / handleCdmaScpMessage",
            InterfaceOutcome::BuildPropertyGate,
            "Same shape as the GSM method and the same two routes, with the same gate.",
        ),
        row(
            InterfaceClass::BinderService,
            "DefaultCellBroadcastService.dump()",
            InterfaceOutcome::ConfigOnly,
            "Reachable via `dumpsys`, and read-only: it prints the default CB receiver package and \
             the handlers' local logs.",
        ),
        row(
            InterfaceClass::ContentProvider,
            "com.android.cellbroadcastservice.CellBroadcastProvider (query/insert/update/delete)",
            InterfaceOutcome::ConfigOnly,
            "Implements query/insert/update/delete over the message history and no call(). \
             A write would create a history row, not an alert — the row is written downstream of \
             the alert path, by GsmCellBroadcastHandler.handleBroadcastSms.",
        ),
        // -- The platform ITelephony service --------------------------------------------------
        row(
            InterfaceClass::TelephonyBinder,
            "ITelephony.getCellBroadcastIdRanges / setCellBroadcastIdRanges",
            InterfaceOutcome::SignatureGate,
            "The only Cell Broadcast methods on the phone service. Both enforce \
             android.permission.MODIFY_CELL_BROADCASTS, a signature permission, and both only \
             configure which channel ranges are enabled. They cannot carry a message.",
        ),
        row(
            InterfaceClass::TelephonyBinder,
            "ITelephony.updateEmergencyNumberListTestMode",
            InterfaceOutcome::SignatureGate,
            "A genuine test-mode setter, but it manipulates the emergency *number* database, not \
             Cell Broadcast, and it is permission-gated.",
        ),
        row(
            InterfaceClass::TelephonyBinder,
            "ITelephony.getEmergencyCallbackMode / startEmergencyCallbackMode",
            InterfaceOutcome::SignatureGate,
            "Emergency-related, unrelated to Cell Broadcast, and signature-gated.",
        ),
        // -- system_server --------------------------------------------------------------------
        row(
            InterfaceClass::BinderService,
            "system_server: CellBroadcastObserver (com.att.iqi)",
            InterfaceOutcome::ConfigOnly,
            "The one place services.jar touches SmsCbMessage. It is an observer that reports CB \
             activity to the AT&T IQI diagnostics library; it consumes messages and cannot \
             originate one.",
        ),
        row(
            InterfaceClass::BinderService,
            "system_server: any service publishing a CB injection method",
            InterfaceOutcome::Absent,
            "A sweep of services.jar for SmsCbMessage construction and for the alert actions found \
             no producer. The single SmsCbMessage reference in services.jar is the AT&T observer \
             above.",
        ),
        // -- OEM components -------------------------------------------------------------------
        row(
            InterfaceClass::OemComponent,
            "com.sec.bcservice (BCService)",
            InterfaceOutcome::ConfigOnly,
            "Named BC, is not Cell Broadcast. A tcpdump/issue-tracker logging service on a unix \
             socket; its only action is com.sec.android.ISSUE_TRACKER_ONOFF, signatureOrSystem.",
        ),
        row(
            InterfaceClass::OemComponent,
            "com.sec.android.app.parser (DRParser) *#*#2627#*#*",
            InterfaceOutcome::ConfigOnly,
            "The keystring router. It rewrites 2627 to the protected SECRET_CODE broadcast, whose \
             handler only flips the testing-mode display filter. It never constructs a message.",
        ),
        row(
            InterfaceClass::OemComponent,
            "com.android.providers.telephony (SecTelephonyProvider) #CMAS# rows",
            InterfaceOutcome::ConfigOnly,
            "Samsung stores emergency alerts as SMS rows whose address is '#CMAS#' / '#CMAS#Test' / \
             '#Emergency Alert#Amber', with the literal cmas table and address LIKE '#CMAS#%' \
             cleanup. These are the SMS database's representation of an alert, written by the \
             alert path. There is no exporter that turns a caller-supplied row into an alert.",
        ),
        row(
            InterfaceClass::OemComponent,
            "com.samsung.android.telephony.SemSmsCbMessage",
            InterfaceOutcome::ConfigOnly,
            "A read-only Parcelable wrapper over SmsCbMessage: every method is a getter.",
        ),
        row(
            InterfaceClass::OemComponent,
            "SecFactoryPhoneTest / ModemServiceMode / serviceModeApp_FB / FactoryTestProvider / SVCAgent",
            InterfaceOutcome::ConfigOnly,
            "Test activities, RMS and keystring interfaces, provider reads. None constructs an \
             SmsCbMessage or calls CellBroadcastAlertService.",
        ),
        row(
            InterfaceClass::OemComponent,
            "com.sec.android.emergencylauncher (EmergencyLauncher)",
            InterfaceOutcome::ConfigOnly,
            "Listens for the ETWS state flag to raise the emergency UI. A consumer of an alert that \
             already arrived, not a producer.",
        ),
        row(
            InterfaceClass::OemComponent,
            "com.samsung.rmt_exercise",
            InterfaceOutcome::Absent,
            "Does not exist in this build. It was the most plausible OEM remote-exercise injector \
             and it is not shipped.",
        ),
        // -- State-only surfaces --------------------------------------------------------------
        row(
            InterfaceClass::StateOnly,
            "persist.cellbroadcast.message_filter",
            InterfaceOutcome::ConfigOnly,
            "Documented 'for testing use' and it only removes messages from consideration. It \
             cannot add one, and persist.* is not writable by shell.",
        ),
        row(
            InterfaceClass::StateOnly,
            "Settings.Global CELL_BROADCAST_TEST_ALERT_ENABLED / Samsung siminfo toggles",
            InterfaceOutcome::ConfigOnly,
            "enable_cmas_test_alerts and enable_etws_test_alerts are receive-side display \
             preferences consulted when a message has already arrived.",
        ),
    ]
}

/// The candidates whose only obstacle is a build property rather than a permission.
///
/// This is the set that matters for the product: these are the interfaces that would work on a
/// `userdebug` device, and they are the ones the trigger engine targets.
pub fn property_gated_injectors() -> Vec<InterfaceCandidate> {
    interface_candidates()
        .into_iter()
        .filter(|candidate| candidate.outcome == InterfaceOutcome::BuildPropertyGate)
        .collect()
}

/// A sentence stating the non-broadcast surface, generated from [`interface_candidates`].
pub fn interface_summary() -> String {
    let all = interface_candidates();
    let count = |outcome: InterfaceOutcome| all.iter().filter(|c| c.outcome == outcome).count();
    let absent = count(InterfaceOutcome::Absent);
    let gated = count(InterfaceOutcome::BuildPropertyGate);
    let config = count(InterfaceOutcome::ConfigOnly);
    let signature = count(InterfaceOutcome::SignatureGate);
    format!(
        "{} non-broadcast interfaces were enumerated across the shell-command layer, Binder \
         services, the platform ITelephony service, system_server and the OEM components. {} are \
         absent from the A35 build (including any `cmd cellbroadcast` verb and the MockModem \
         service), {} are behind a signature permission, and {} only configure, filter or read. \
         {} -- the ICellBroadcastService handlers -- do construct an SmsCbMessage and do feed the \
         genuine alert service, and the only thing standing in front of them is ro.debuggable.",
        all.len(),
        absent,
        signature,
        config,
        gated
    )
}

// ---------------------------------------------------------------------------------------------
// Samsung-specific native surface, read out of the A35's own firmware
// ---------------------------------------------------------------------------------------------

/// How a claim in this module was established.
///
/// The distinction between `ProvenOnFirmware` and `UnprovenHere` is the whole point: a claim that
/// was read out of the device's shipped files is a different kind of statement from one that was
/// inferred from AOSP, and the two must not be printed with the same confidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FirmwareEvidence {
    /// Read out of the shipped firmware image for the exact build named in `references`.
    ProvenOnFirmware,
    /// Read out of AOSP source; not yet checked against a device.
    ProvenOnAosp,
    /// Not established either way.
    UnprovenHere,
}

impl FirmwareEvidence {
    pub fn label(self) -> &'static str {
        match self {
            FirmwareEvidence::ProvenOnFirmware => "PROVEN (firmware)",
            FirmwareEvidence::ProvenOnAosp => "PROVEN (AOSP)",
            FirmwareEvidence::UnprovenHere => "UNPROVEN",
        }
    }
}

/// One fact about the native Surface a device exposes, with its provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FirmwareFact {
    pub subject: &'static str,
    pub claim: &'static str,
    pub evidence: FirmwareEvidence,
    pub reference: &'static str,
}

/// The build these facts were read from.
const A35_FIRMWARE_BUILD: &str = "A356BXXS4AYD1 / UP1A.231005.007, Android 14, One UI 6.1, \
                                  ro.build.type=user, ro.build.tags=release-keys";

/// The one question this whole investigation reduces to, and the firmware-proven answer.
///
/// The AOSP test receiver (`GsmCbTestBroadcastReceiver`) is registered inside
/// `GsmInboundSmsHandler` only when `ro.debuggable == 1`. That is a build property. On a retail A35
/// it is `0`, so the receiver is never constructed and there is no non-root path through it. The
/// point of reading Samsung's own image was to check whether Samsung shipped *some other* entry
/// point beside the AOSP one — a diagnostic receiver, a test service, a privileged helper, a
/// mis-exported component. The enumeration below is that check, and it is what makes this answer a
/// result rather than an assumption.
pub fn samsung_firmware_facts() -> Vec<FirmwareFact> {
    let row = |subject, claim, evidence, reference| FirmwareFact {
        subject,
        claim,
        evidence,
        reference,
    };
    vec![
        row(
            "Cell Broadcast implementation",
            "The A35 ships Google's module (com.google.android.cellbroadcast, APEX \
             341410010) unmodified. There is no Samsung-forked CellBroadcastReceiver; the Samsung \
             packages only overlay it.",
            FirmwareEvidence::ProvenOnFirmware,
            "system/apex/com.google.android.cellbroadcast_compressed.apex",
        ),
        row(
            "Samsung RRO overlay",
            "com.google.android.overlay.modules.cellbroadcastreceiver (target \
             CellBroadcastCustomization) changes display strings, one theme and three UI booleans \
             (show_alert_dialog_with_notification, show_alert_speech_setting, \
             show_presidential_alerts_settings). It does not touch allow_testing_mode_on_user_build, \
             show_test_settings, the channel range arrays, or any receiver.",
            FirmwareEvidence::ProvenOnFirmware,
            "product/overlay/CellBroadcastConfigOverlay.apk (public.xml)",
        ),
        row(
            "Samsung RRO overlay (service)",
            "com.google.android.overlay.modules.cellbroadcastservice (target \
             CellBroadcastServiceCustomization) sets cross_sim_duplicate_detection=false and \
             config_area_info_receiver_packages={com.android.systemui}. No receiver, service or \
             permission is added.",
            FirmwareEvidence::ProvenOnFirmware,
            "product/overlay/CellBroadcastServiceOverlay.apk (public.xml)",
        ),
        row(
            "AOSP telephony test receiver",
            "Samsung did not modify GsmInboundSmsHandler or CdmaInboundSmsHandler. The test receiver \
             is still registered only when ro.debuggable==1, still RECEIVER_EXPORTED with no \
             permission, and still feeds mCellBroadcastServiceManager.sendGsmMessageToHandler.",
            FirmwareEvidence::ProvenOnFirmware,
            "system/framework/telephony-common.jar; GsmInboundSmsHandler.java:45-49,70",
        ),
        row(
            "AOSP gate stability (context)",
            "The gate Samsung carried unmodified is stable across AOSP android14/15/16: the test \
             receiver is registered on ro.debuggable==1, RECEIVER_EXPORTED, no permission. This is \
             the yardstick the A35 was compared against, and it is AOSP-derived rather than read \
             from the image.",
            FirmwareEvidence::ProvenOnAosp,
            "frameworks/opt/telephony GsmInboundSmsHandler (android14/15/16-release)",
        ),
        row(
            "Build type",
            "ro.build.type=user, ro.build.tags=release-keys, ro.debuggable=0 and \
             ro.force.debuggable=0. The AOSP test receiver is therefore never registered.",
            FirmwareEvidence::ProvenOnFirmware,
            "system/build.prop",
        ),
        row(
            "Protected broadcasts",
            "android.provider.Telephony.SMS_CB_RECEIVED, \
             android.provider.action.SMS_EMERGENCY_CB_RECEIVED, \
             android.provider.Telephony.SMS_SERVICE_CATEGORY_PROGRAM_DATA_RECEIVED and \
             android.telephony.action.SECRET_CODE are all in the device's own \
             <protected-broadcast> list, so AMS refuses them before broadcast resolution.",
            FirmwareEvidence::ProvenOnFirmware,
            "system/framework/framework-res.apk AndroidManifest.xml",
        ),
        row(
            "Secret code 2627",
            "*#*#2627#*#* is routed by DRParser to a protected broadcast \
             (android.telephony.action.SECRET_CODE, forced for exactly 2627 and 4636), which the \
             platform refuses from a shell caller. The receiver's handler only calls \
             setTestingMode(!isTestingMode(...)) — it toggles a display filter and constructs no \
             message.",
            FirmwareEvidence::ProvenOnFirmware,
            "DRParser.apk ParseService.java:139; CellBroadcastReceiver.java:83-100",
        ),
        row(
            "Testing-mode gate on this build",
            "ro.debuggable=0, but allow_testing_mode_on_user_build=true in the shipped bools.xml, \
             so the 2627 toggle is live on the A35. It opens the test-mode channel range; it does \
             not originate a message.",
            FirmwareEvidence::ProvenOnFirmware,
            "GoogleCellBroadcastApp.apk res/values/bools.xml",
        ),
        row(
            "BCService",
            "com.sec.bcservice is not Cell Broadcast despite the name. BroadcastService is a \
             tcpdump/issue-tracker logging service on a unix socket. Its only broadcast action is \
             com.sec.android.ISSUE_TRACKER_ONOFF, guarded by signatureOrSystem.",
            FirmwareEvidence::ProvenOnFirmware,
            "system/priv-app/BCService/BCService.apk",
        ),
        row(
            "SemSmsCbMessage",
            "Samsung's CB API (com.samsung.android.telephony.SemSmsCbMessage) is a read-only \
             Parcelable wrapper over android.telephony.SmsCbMessage: every method is a getter. It \
             has no constructor from a PDU and no path back into the alert service.",
            FirmwareEvidence::ProvenOnFirmware,
            "system/framework/telephony-common.jar",
        ),
        row(
            "TeleService",
            "Samsung's phone app adds no Cell Broadcast action. Its only CB surface is \
             get/setCellBroadcastIdRanges in PhoneInterfaceManager, both requiring \
             android.permission.MODIFY_CELL_BROADCASTS (signature) — these configure ranges and \
             cannot inject a message.",
            FirmwareEvidence::ProvenOnFirmware,
            "system/priv-app/TeleService/TeleService.apk",
        ),
        row(
            "Shell / binder test surface",
            "No `cmd cellbroadcast` verb, no onShellCommand handler, and no CB test Binder method \
             exists in the shipped telephony or CB code. The exported DefaultCellBroadcastService \
             requires the signature permission BIND_CELL_BROADCAST_SERVICE.",
            FirmwareEvidence::ProvenOnFirmware,
            "telephony-common.jar; GoogleCellBroadcastServiceModule.apk manifest",
        ),
        row(
            "Factory / diagnostic surface",
            "SecFactoryPhoneTest, ModemServiceMode, serviceModeApp_FB, FactoryTestProvider and \
             SVCAgent expose test activities, RMS/keystring interfaces and provider reads, but none \
             of them constructs an SmsCbMessage or calls CellBroadcastAlertService. No \
             com.samsung.rmt_exercise (remote exercise) package exists in this build.",
            FirmwareEvidence::ProvenOnFirmware,
            "system/priv-app/{SecFactoryPhoneTest,ModemServiceMode,serviceModeApp_FB,FactoryTestProvider,SVCAgent}",
        ),
        row(
            "Injection-token sweep",
            "A sweep of the shipped Samsung components and framework jars for \
             TEST_TRIGGER_CELL_BROADCAST, pdu_string, sendGsmMessageToHandler, \
             handleCellBroadcastIntent, SmsCbMessage construction tokens and \
             CellBroadcastAlertService produced no hit outside Google's own module. There is no \
             OEM injector to find.",
            FirmwareEvidence::ProvenOnFirmware,
            "band analysis of every package in the A35 image",
        ),
        row(
            "Receiver package on the A35",
            "AOSP's manifest component name is com.android.cellbroadcastreceiver.CellBroadcastReceiver \
             and the alert services are exported=false. A prior revision of this project also looked \
             for com.samsung.android.cellbroadcastreceiver, which would be the name on older \
             One UI builds; it is absent here.",
            FirmwareEvidence::UnprovenHere,
            "not present on A356BXXS4AYD1; may exist on other One UI builds",
        ),
    ]
}

/// The single firmware-proven conclusion about the A35, stated once so prose cannot drift from the
/// registry.
pub fn samsung_native_entrypoint_conclusion() -> String {
    let facts = samsung_firmware_facts();
    let proven = facts
        .iter()
        .filter(|fact| fact.evidence == FirmwareEvidence::ProvenOnFirmware)
        .count();
    format!(
        "The native Samsung surface was read from {A35_FIRMWARE_BUILD}. {proven} of {} recorded \
         facts were proven directly from the shipped files. Samsung ships Google's Cell Broadcast \
         module unmodified and adds no injector: no OEM receiver, service, provider, Binder method \
         or shell verb constructs a Cell Broadcast message or enters CellBroadcastAlertService \
         except the AOSP telephony test receiver, which this build's ro.debuggable=0 never \
         registers.",
        facts.len()
    )
}

// ---------------------------------------------------------------------------------------------
// Cell Broadcast PDU construction
// ---------------------------------------------------------------------------------------------

/// GSM 03.38 basic character set, by septet value.
///
/// Index 0x1B is the escape to the extension table and is not representable as a single septet, so
/// encoding refuses characters that need it rather than emitting a lone escape.
const GSM7_BASIC: &str = concat!(
    "@\u{a3}$\u{a5}\u{e8}\u{e9}\u{f9}\u{ec}\u{f2}\u{c7}\n\u{d8}\u{f8}\r\u{c5}\u{e5}",
    "\u{394}_\u{3a6}\u{393}\u{39b}\u{3a9}\u{3a0}\u{3a8}\u{3a3}\u{398}\u{39e}\u{1b}\u{c6}\u{e6}\u{df}\u{c9}",
    " !\"#\u{a4}%&'()*+,-./",
    "0123456789:;<=>?",
    "\u{a1}ABCDEFGHIJKLMNO",
    "PQRSTUVWXYZ\u{c4}\u{d6}\u{d1}\u{dc}\u{a7}",
    "\u{bf}abcdefghijklmno",
    "pqrstuvwxyz\u{e4}\u{f6}\u{f1}\u{fc}\u{e0}",
);

/// Map one character to its GSM 7-bit septet, or `None` if it is outside the basic table.
pub fn gsm7_septet(character: char) -> Option<u8> {
    let position = GSM7_BASIC.chars().position(|candidate| candidate == character)?;
    let septet = position as u8;
    if septet == 0x1B {
        return None;
    }
    Some(septet)
}

/// Encode text to GSM 7-bit septets, refusing anything outside the basic alphabet.
///
/// Refusing rather than substituting is the honest behaviour: silently replacing characters would
/// mean the alert body on the phone differs from the body the operator typed, and the whole point
/// of this tool is that the operator can trust what they see.
pub fn encode_gsm7(text: &str) -> Result<Vec<u8>, String> {
    text.chars()
        .map(|character| {
            gsm7_septet(character).ok_or_else(|| {
                format!("character {character:?} is not in the GSM 7-bit default alphabet")
            })
        })
        .collect()
}

/// Pack septets into octets, least-significant bit first.
///
/// 3GPP TS 23.041 9.4.1.2: "The bits within these octets are numbered 0 to 7; bit 0 is the low
/// order bit and is transmitted first."
pub fn pack_septets(septets: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity((septets.len() * 7).div_ceil(8));
    let mut accumulator: u32 = 0;
    let mut bits: u32 = 0;
    for &septet in septets {
        accumulator |= u32::from(septet & 0x7F) << bits;
        bits += 7;
        while bits >= 8 {
            out.push((accumulator & 0xFF) as u8);
            accumulator >>= 8;
            bits -= 8;
        }
    }
    if bits > 0 {
        out.push((accumulator & 0xFF) as u8);
    }
    out
}

/// Reverse of [`pack_septets`].
///
/// The septet count must be supplied: the final octet of a packed run carries padding bits that
/// are indistinguishable from a real septet, so an unpacker without a length would emit one bogus
/// trailing character for every run whose bit count is not a multiple of 7.
pub fn unpack_septets(data: &[u8], septet_count: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(septet_count);
    let mut accumulator: u32 = 0;
    let mut bits: u32 = 0;
    for &byte in data {
        if out.len() == septet_count {
            break;
        }
        accumulator |= u32::from(byte) << bits;
        bits += 8;
        while bits >= 7 && out.len() < septet_count {
            out.push((accumulator & 0x7F) as u8);
            accumulator >>= 7;
            bits -= 7;
        }
    }
    out
}

/// The six-octet header of a GSM Cell Broadcast message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CbHeader {
    pub serial_number: u16,
    pub message_id: u16,
    pub data_coding_scheme: u8,
    pub page_parameter: u8,
}

impl CbHeader {
    pub fn message_id_label(&self) -> &'static str {
        match self.message_id {
            MESSAGE_ID_ETWS_TEST => "ETWS TEST (0x1103)",
            MESSAGE_ID_ETWS_EARTHQUAKE => "ETWS EARTHQUAKE WARNING (0x1100)",
            0x1101 => "ETWS TSUNAMI WARNING (0x1101)",
            0x1102 => "ETWS EARTHQUAKE+TSUNAMI (0x1102)",
            0x1104 => "ETWS OTHER EMERGENCY (0x1104)",
            _ => "UNKNOWN MESSAGE ID",
        }
    }
}

/// Data Coding Scheme for GSM 7-bit default alphabet, uncompressed (3GPP TS 23.038 coding group
/// `0001`, character set `00`).
const DCS_GSM7: u8 = 0x11;

/// Parse the fixed header out of a GSM Cell Broadcast PDU.
pub fn parse_cb_header(pdu: &[u8]) -> Option<CbHeader> {
    if pdu.len() < 6 {
        return None;
    }
    Some(CbHeader {
        serial_number: u16::from_be_bytes([pdu[0], pdu[1]]),
        message_id: u16::from_be_bytes([pdu[2], pdu[3]]),
        data_coding_scheme: pdu[4],
        page_parameter: pdu[5],
    })
}

/// Build a GSM Cell Broadcast PDU carrying an ETWS test message.
///
/// Layout per 3GPP TS 23.041 9.4.1.2:
/// ```text
/// octets 1-2  Serial Number
/// octets 3-4  Message Identifier
/// octet  5    Data Coding Scheme
/// octet  6    Page Parameter
/// octets 7-N  Content of Message
/// ```
/// The content is packed 7 bits per character and bit-filled to an octet boundary. No page padding
/// is appended: the reference `pdu_string` in `GsmInboundSmsHandler` is itself shorter than the
/// 88-octet radio page, so the parser reads the content length from the PDU rather than the page.
pub fn etws_test_pdu(body: &str, serial_number: u16) -> Result<Vec<u8>, String> {
    cb_pdu(MESSAGE_ID_ETWS_TEST, body, serial_number)
}

/// Build a Cell Broadcast PDU for any identifier in the [`ALERT_CHANNELS`] catalogue.
///
/// The channel is a parameter rather than a constant because the identifier decides whether AOSP
/// raises the alert at all: `CellBroadcastAlertService.isChannelEnabled` consults a different user
/// preference per channel, and the ETWS test channel (`0x1103`) that this tool used to hardcode is
/// disabled by default. See [`ALERT_CHANNELS`].
///
/// The body must still open with `TEST`, which keeps every PDU this tool can produce obviously
/// synthetic on the receiving handset.
pub fn cb_pdu(message_id: u16, body: &str, serial_number: u16) -> Result<Vec<u8>, String> {
    if !body.starts_with("TEST") {
        return Err("refusing to build an alert whose body does not begin with TEST".to_string());
    }
    if alert_channel(message_id).is_none() {
        return Err(format!(
            "message identifier 0x{message_id:04X} is not in the channel catalogue; \
             register it in ALERT_CHANNELS with its receive-side gate before emitting it"
        ));
    }
    let septets = encode_gsm7(body)?;
    if septets.len() > 93 {
        return Err(format!(
            "body is {} characters; one Cell Broadcast page carries at most 93",
            septets.len()
        ));
    }

    let mut pdu = Vec::with_capacity(6 + septets.len());
    pdu.extend_from_slice(&serial_number.to_be_bytes());
    pdu.extend_from_slice(&message_id.to_be_bytes());
    pdu.push(DCS_GSM7);
    pdu.push(0x01); // page 1 of 1
    pdu.extend_from_slice(&pack_septets(&septets));
    Ok(pdu)
}

/// The AOSP reference PDU from the `GsmInboundSmsHandler` doc comment.
///
/// Kept as a test vector: it proves the header parser reads the layout AOSP itself uses. The doc
/// comment wraps the hex string across three lines at 71/88/13 characters, and 71 is odd, so the
/// visual wrap splits one byte in half; the canonical pdu_string is the 172-character
/// concatenation of all three.
pub const AOSP_REFERENCE_PDU_HEX: &str = concat!(
    "0000110011010D0A5BAE57CE770C531790E85C716CBF3044573065B9306757309707767A751F30025F3730",
    "4463FA308C306B5099304830664E0B30553044FF086C178C615E81FF090000000000000000000000000000",
);

/// Why the AOSP test entry point is or is not usable, in operator-facing terms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TestEntryPoint {
    pub available: State,
    pub reason: String,
}

/// Decide whether the AOSP test-injection action can reach the telephony test receiver.
///
/// This is the single most important honest answer the tool gives about a retail phone. The
/// receiver inside `GsmInboundSmsHandler` is registered only when `ro.debuggable == 1`, so on
/// production firmware the broadcast is *accepted by `am`* and then does nothing at all. An
/// accepted command is not a delivered alert, and the tool has to say so before the operator
/// spends an evening chasing a message that was never going to arrive.
pub fn assess_test_entrypoint(debuggable: &str, receiver_package: Option<&str>) -> TestEntryPoint {
    if receiver_package.is_none() {
        return TestEntryPoint {
            available: State::NotPresent,
            reason: "No Cell Broadcast receiver package was found on this device.".to_string(),
        };
    }
    match debuggable_permits_test_entrypoint(debuggable) {
        Some(true) => TestEntryPoint {
            available: State::Granted,
            reason: format!(
                "ro.debuggable=1, so the AOSP test receiver in GsmInboundSmsHandler is registered. \
                 {} is present. The test broadcast can reach the telephony pipeline.",
                receiver_package.unwrap_or("the receiver")
            ),
        },
        Some(false) => TestEntryPoint {
            available: State::Denied,
            reason: "ro.debuggable=0. This is a production build, so the AOSP test receiver is \
                     never registered and the test broadcast is accepted by `am` and then \
                     discarded. This is a build property, not a permission, so it cannot be \
                     granted on the device."
                .to_string(),
        },
        None => TestEntryPoint {
            available: State::Unknown,
            reason: "ro.debuggable could not be read, so whether the AOSP test receiver exists is \
                     unknown. Do not assume it is absent."
                .to_string(),
        },
    }
}

/// What a logcat capture proves about the platform Cell Broadcast pipeline.
///
/// Each field is a separate, strictly stronger claim. The old code jumped from "a package exists"
/// to "Cell Broadcast supported"; these are the distinctions that jump skipped.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PlatformEvidence {
    /// The AOSP test receiver reported receiving the intent.
    pub test_receiver_accepted: bool,
    /// A `SmsCbMessage` was constructed from the PDU.
    pub message_constructed: bool,
    /// The Cell Broadcast service or handler processed the message.
    pub service_reached: bool,
    /// The Cell Broadcast receiver ran.
    pub receiver_processed: bool,
    /// `CellBroadcastAlertService` started on a Cell-Broadcast action.
    ///
    /// Distinct from `alert_ui_requested`: the service starting means the message entered the alert
    /// service, where the channel-range, testing-mode and language gates run. Claiming the UI from
    /// this would skip exactly the step that most often drops a test alert.
    pub alert_service_started: bool,
    /// Android requested its own alert UI.
    pub alert_ui_requested: bool,
    /// A log line shows the platform *deliberately dropped* the message, and why.
    ///
    /// This is the difference between "nothing happened and we do not know why" and "the platform
    /// told us why". `sent` is the raw marker so the report can quote the device rather than
    /// paraphrase it. When this is set, an absent `alert_ui_requested` is an explained outcome, not
    /// an unresolved one.
    pub suppression: Option<Suppression>,
}

/// A platform decision to discard the message, read from a log line AOSP actually emits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Suppression {
    /// Which gate closed.
    pub gate: SuppressionGate,
    /// The matching log text, verbatim.
    pub sent: String,
}

impl Suppression {
    /// A sentence an operator can act on, naming the gate and what it means.
    pub fn explain(&self) -> String {
        match self.gate {
            SuppressionGate::DisabledByOem => "the OEM master switch for Cell Broadcast is on \
                (`config_disable_all_cb_messages`), so every Cell Broadcast message is dropped \
                before it reaches the handler. This is a framework resource, not a permission; \
                nothing on this desktop can change it."
                .to_string(),
            SuppressionGate::TestModeRequired => "the message landed on a channel range marked \
                test-mode-only and the device is not in testing mode, so the alert was filtered. \
                Testing mode is toggled on the phone itself."
                .to_string(),
            SuppressionGate::ChannelDisabled => "the channel carrying this message is not enabled \
                in the device's Cell Broadcast settings, so the alert was discarded."
                .to_string(),
            SuppressionGate::LanguageMismatch => "the message's declared language does not match \
                the device language, so the alert was filtered."
                .to_string(),
            SuppressionGate::ContentFilter => "a device-configured content filter matched the \
                message text, so the alert was discarded."
                .to_string(),
        }
    }
}

/// The specific platform gate that discarded the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SuppressionGate {
    DisabledByOem,
    TestModeRequired,
    ChannelDisabled,
    LanguageMismatch,
    ContentFilter,
}

impl PlatformEvidence {
    /// The strongest stage this evidence supports, and nothing stronger.
    ///
    /// The ladder here is deliberately longer than the number of boolean fields: an alert service
    /// that started on a Cell-Broadcast action is a stronger claim than the receiver running, and a
    /// message the platform *deliberately dropped* is a different claim again. Reporting the drop as
    /// "the UI was reached" would be the false-success shape this project exists to catch.
    pub fn stage(&self) -> CapabilityStage {
        if self.was_suppressed() {
            // The message reached the alert service and the platform then discarded it. That is
            // strictly more than "the receiver ran" and strictly less than "the UI appeared", and
            // since the platform told us which gate closed, it belongs at the alert-service rung.
            return CapabilityStage::AlertServiceReached;
        }
        if self.alert_ui_requested {
            CapabilityStage::SystemUiReached
        } else if self.alert_service_started {
            CapabilityStage::AlertServiceReached
        } else if self.receiver_processed {
            CapabilityStage::ReceiverProcessed
        } else if self.service_reached {
            CapabilityStage::CellBroadcastServiceReached
        } else if self.message_constructed || self.test_receiver_accepted {
            CapabilityStage::TestEntryPointAccepted
        } else {
            CapabilityStage::TestEntryPointDiscovered
        }
    }

    /// True when the capture shows the pipeline actually ran.
    pub fn pipeline_ran(&self) -> bool {
        self.test_receiver_accepted
            || self.message_constructed
            || self.service_reached
            || self.receiver_processed
            || self.alert_service_started
            || self.alert_ui_requested
    }

    /// True when the platform was witnessed discarding the message.
    ///
    /// A suppressed message is a definite negative result with a stated cause, so a caller must not
    /// keep treating it as "we could not tell". Saying `UNKNOWN` here would hide the one thing the
    /// device was most explicit about.
    pub fn was_suppressed(&self) -> bool {
        self.suppression.is_some()
    }

    /// The strongest rung of [`VerificationStage`] this capture proves, and nothing stronger.
    ///
    /// A suppressed run can never reach [`VerificationStage::Success`]: the platform told us the
    /// message arrived and that it was then dropped, so the honest answer is that the alert
    /// pipeline ran up to the gate and stopped there.
    pub fn verification_stage(&self) -> VerificationStage {
        if self.was_suppressed() {
            return VerificationStage::AlertServiceDetected;
        }
        if self.alert_ui_requested {
            VerificationStage::Success
        } else if self.alert_service_started {
            VerificationStage::AlertServiceDetected
        } else if self.receiver_processed || self.service_reached || self.message_constructed {
            VerificationStage::CellBroadcastProcessingDetected
        } else if self.test_receiver_accepted {
            VerificationStage::TelephonyActivityDetected
        } else {
            // The trigger was sent (this evidence only exists after a send) but nothing downstream
            // was observed. That is UNKNOWN, not failure, and certainly not success.
            VerificationStage::TriggerSent
        }
    }
}

/// Log text that shows the platform dropped the message, and which gate did it.
///
/// These are the exact strings AOSP emits, read from `CellBroadcastServiceManager`,
/// `CellBroadcastAlertService` and the carrier-config path. Each one is a *negative* marker: it
/// proves the message arrived and was then deliberately discarded. They are matched before the
/// positive markers so a suppression is never masked by a tag that also appeared earlier in the
/// pipeline.
const SUPPRESSION_MARKERS: &[(&str, SuppressionGate)] = &[
    ("GSM CB message ignored - CB messages disabled by OEM", SuppressionGate::DisabledByOem),
    ("CDMA CB message ignored - CB messages disabled by OEM", SuppressionGate::DisabledByOem),
    ("CDMA SCP CB message ignored - CB messages disabled by OEM", SuppressionGate::DisabledByOem),
    ("ignoring the alert due to not in testing mode", SuppressionGate::TestModeRequired),
    ("ignoring the alert due to configured channels was marked", SuppressionGate::ChannelDisabled),
    ("ignoring the alert due to language mismatch", SuppressionGate::LanguageMismatch),
    ("Skipped message due to filter", SuppressionGate::ContentFilter),
];

/// Scan a logcat capture for platform-path evidence.
pub fn scan_platform_logcat(logcat: &str) -> PlatformEvidence {
    let matches = |markers: &[&str]| markers.iter().any(|marker| logcat.contains(marker));
    PlatformEvidence {
        test_receiver_accepted: matches(MARKER_TEST_RECEIVER),
        message_constructed: matches(MARKER_MESSAGE_CONSTRUCTED),
        service_reached: matches(MARKER_SERVICE),
        receiver_processed: matches(MARKER_RECEIVER),
        alert_service_started: matches(MARKER_ALERT_SERVICE),
        alert_ui_requested: matches(MARKER_ALERT_UI),
        suppression: first_suppression(logcat),
    }
}

/// The first suppression marker present in the capture, if any.
fn first_suppression(logcat: &str) -> Option<Suppression> {
    SUPPRESSION_MARKERS
        .iter()
        .find(|(marker, _)| logcat.contains(marker))
        .map(|(marker, gate)| Suppression {
            gate: *gate,
            sent: (*marker).to_string(),
        })
}

/// Log markers that indicate each stage of the AOSP pipeline.
///
/// AOSP uses a separate log line for each step, so these are distinct claims rather than one
/// regex over a single line.
///
/// These markers were re-checked against the AOSP sources this session, because the earlier set
/// contained a marker that could fire on an action the Cell Broadcast app **rejects**. The app's
/// receiver tag is literally `"CellBroadcastReceiver"`, and its `onReceive` logs
/// `"onReceive() unexpected action <action>"` for anything it does not handle — so a stray broadcast
/// to that component could be read as proof the Cell Broadcast alert path ran. Matching the tag
/// alone is not evidence. The negative markers in `SUPPRESSION_MARKERS` are the opposite case: each
/// one is emitted only *after* the message reached the platform and was deliberately dropped, so
/// they are the most trustworthy lines in the capture.
const MARKER_TEST_RECEIVER: &[&str] = &["Received test intent action"];
const MARKER_MESSAGE_CONSTRUCTED: &[&str] =
    &["SmsCbMessage", "handleGsmCellBroadcastSms", "CellBroadcastMessage"];
const MARKER_SERVICE: &[&str] =
    &["CellBroadcastService", "CellBroadcastHandler", "GsmCellBroadcastHandler"];
/// Positive evidence that the Cell Broadcast app *processed the message*, not merely that its
/// process existed.
///
/// The app's only unconditional receive-side log is `CellBroadcastReceiver: onReceive <intent>`
/// (guarded by a `DBG` flag that is hardcoded `true`), and it fires for **every** action the
/// receiver is handed — including the `onReceive() unexpected action` case below. So the bare tag
/// is not evidence, and neither is the bare action string (the alert service logs the same action
/// from its own tag). The marker is the receiver's own intent dump carrying a Cell-Broadcast action,
/// which only appears when that receiver was entered with that action. If an OEM reformats the
/// intent dump the marker misses and the stage stays unclaimed, which is the safe direction.
const MARKER_RECEIVER: &[&str] = &[
    "CellBroadcastReceiver: onReceive Intent { act=android.provider.Telephony.SMS_CB_RECEIVED",
    "CellBroadcastReceiver: onReceive Intent { act=android.provider.action.SMS_EMERGENCY_CB_RECEIVED",
];
/// Evidence that the message entered `CellBroadcastAlertService`.
///
/// `CBAlertService: onStartCommand` is emitted when the alert service is started on a
/// Cell-Broadcast action, which is *before* its channel-range and testing-mode gates run. It is
/// therefore the right marker for the "alert service reached" rung and the wrong one for "the alert
/// was presented" -- that is [`MARKER_ALERT_UI`]'s job.
const MARKER_ALERT_SERVICE: &[&str] = &["CBAlertService: onStartCommand"];

/// Evidence that Android moved from "the alert service is running" to "present this alert".
///
/// `openEmergencyAlertNotification` is the call that selects the presentation, and it is the only
/// marker here that survives the alert service's own gates. `CBAlertService: onStartCommand` is
/// deliberately *not* in this list any more: it fires before the gates, so a run the platform then
/// suppressed would otherwise be reported as a presented alert.
const MARKER_ALERT_UI: &[&str] = &["openEmergencyAlertNotification"];

/// The complete, honest capability picture for one device.
#[derive(Debug, Clone, Serialize)]
pub struct PlatformProbe {
    pub build_type: Option<String>,
    pub debuggable: Option<String>,
    pub test_entrypoint: TestEntryPoint,
    pub cellbroadcast_candidates: Vec<String>,
    pub cellbroadcast_package: Option<String>,
    pub receiver_declared: State,
    pub local_simulator_installed: bool,
    pub post_notifications: State,
    pub full_screen_intent: State,
    pub notifications_enabled: State,
    /// The strongest capability established by evidence, not by inference.
    pub stage: CapabilityStage,
    /// One line the UI can show without overclaiming.
    pub summary: String,
    /// Every command that produced this picture, with its raw result.
    pub evidence: Vec<ProbeEvidence>,
}

/// One probe command and what came back.
#[derive(Debug, Clone, Serialize)]
pub struct ProbeEvidence {
    pub label: String,
    pub command: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// The parsed meaning, or why parsing produced nothing.
    pub parsed: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_exported_test_actions() {
        let samsung_action = format!("{}.TEST_CELL_BROADCAST", "com.samsung");
        let private_action = format!("{}.TEST_EMERGENCY_ALERT", "com.samsung");
        let dump = format!(
            r#"
Package [com.samsung.test] (123):
  Receiver{{abc com.samsung.test/.Receiver}}
    exported=true
    Action: "{samsung_action}"
  Receiver{{def com.samsung.test/.Private}}
    exported=false
    Action: "{private_action}"
"#
        );
        assert_eq!(
            discover_test_actions(&dump),
            vec![samsung_action]
        );
    }

    // -----------------------------------------------------------------------------------------
    // Permission parsing: the bug the operator actually hit
    // -----------------------------------------------------------------------------------------

    /// AOSP and Pixel formatting, permission granted.
    #[test]
    fn parses_a_granted_runtime_permission() {
        let dump = "\
Packages:
  Package [com.tirodz.emergencysimulator] (1a2b3c):
    requested permissions:
      android.permission.POST_NOTIFICATIONS
      android.permission.VIBRATE
    runtime permissions:
      android.permission.POST_NOTIFICATIONS: granted=true, flags=[ USER_SET|USER_SELECTED ]
      android.permission.VIBRATE: granted=true
";
        assert_eq!(parse_post_notifications(dump), State::Granted);
    }

    /// The exact shape that produced the false "granted=false": the permission is *requested* and
    /// never appears again with a grant state. Samsung prints sections in a different order and can
    /// omit the runtime block for an app whose permission has not been evaluated yet.
    #[test]
    fn a_requested_permission_is_not_a_denial() {
        let dump = "\
Packages:
  Package [com.tirodz.emergencysimulator] (1a2b3c):
    requested permissions:
      android.permission.POST_NOTIFICATIONS
    install permissions:
      android.permission.VIBRATE: granted=true
";
        assert_eq!(parse_post_notifications(dump), State::Unknown);
    }

    /// A real denial, with flags, must still be a denial.
    #[test]
    fn parses_an_explicit_denial() {
        let dump = "\
    runtime permissions:
      android.permission.POST_NOTIFICATIONS: granted=false, flags=[ USER_SET|USER_FIXED ]
";
        assert_eq!(parse_post_notifications(dump), State::Denied);
    }

    /// Samsung splits the permission entry across lines and lists the name with a trailing colon.
    #[test]
    fn parses_a_samsung_style_multiline_entry() {
        let dump = "\
  Package [com.tirodz.emergencysimulator] (9f8e7d):
    runtime permissions:
      android.permission.POST_NOTIFICATIONS: granted=true
        flags=[ USER_SET|USER_SELECTED|REVOKE_WHEN_REQUESTED ]
      android.permission.VIBRATE: granted=true
";
        assert_eq!(parse_post_notifications(dump), State::Granted);
    }

    /// The requested block must not win over a later explicit grant.
    #[test]
    fn prefers_the_authoritative_section_over_a_bare_mention() {
        let dump = "\
    requested permissions:
      android.permission.POST_NOTIFICATIONS
    runtime permissions:
      android.permission.POST_NOTIFICATIONS: granted=true
";
        assert_eq!(parse_post_notifications(dump), State::Granted);
    }

    /// A missing package is a distinct answer from a denied permission.
    #[test]
    fn a_missing_package_is_not_present() {
        assert_eq!(
            parse_post_notifications("Unable to find package: com.tirodz.emergencysimulator"),
            State::NotPresent
        );
    }

    /// Silence must never be read as a denial.
    #[test]
    fn an_empty_dump_is_unknown_not_denied() {
        assert_eq!(parse_post_notifications(""), State::Unknown);
        assert_eq!(parse_post_notifications("   \n  "), State::Unknown);
    }

    /// A package that exists but never mentions the permission is not a denial either.
    #[test]
    fn a_dump_without_the_permission_is_not_denied() {
        let dump = "  Package [com.example] (aa):\n    versionCode=1\n";
        assert_eq!(parse_post_notifications(dump), State::NotPresent);
    }

    /// `granted=` with a value that is neither true nor false must not be guessed.
    #[test]
    fn an_unrecognised_grant_value_is_unknown() {
        assert_eq!(parse_granted_flag("granted=maybe"), None);
        assert_eq!(parse_granted_flag("granted="), None);
        assert_eq!(parse_granted_flag("no flag here"), None);
    }

    // -----------------------------------------------------------------------------------------
    // App-ops parsing
    // -----------------------------------------------------------------------------------------

    #[test]
    fn parses_allowed_and_denied_appops() {
        assert_eq!(parse_appop("POST_NOTIFICATION: allow"), State::Granted);
        assert_eq!(parse_appop("POST_NOTIFICATION: deny"), State::Denied);
        assert_eq!(parse_appop("USE_FULL_SCREEN_INTENT: ignore"), State::Denied);
    }

    #[test]
    fn an_unknown_operation_is_not_a_denial() {
        assert_eq!(parse_appop("Unknown operation string: FOO"), State::NotPresent);
        assert_eq!(parse_appop(""), State::Unknown);
        assert_eq!(parse_appop("POST_NOTIFICATION: default"), State::Unknown);
    }

    /// For this specific op the platform default *is* denial, so `default` must mean denied.
    #[test]
    fn full_screen_intent_defaults_to_denied() {
        assert_eq!(parse_full_screen_intent("USE_FULL_SCREEN_INTENT: default"), State::Denied);
        assert_eq!(parse_full_screen_intent("USE_FULL_SCREEN_INTENT: allow"), State::Granted);
        assert_eq!(parse_full_screen_intent("USE_FULL_SCREEN_INTENT: deny"), State::Denied);
        assert_eq!(parse_full_screen_intent(""), State::Unknown);
    }

    // -----------------------------------------------------------------------------------------
    // Build capability
    // -----------------------------------------------------------------------------------------

    #[test]
    fn only_a_debuggable_build_exposes_the_test_entry_point() {
        assert_eq!(debuggable_permits_test_entrypoint("1"), Some(true));
        assert_eq!(debuggable_permits_test_entrypoint("0"), Some(false));
        assert_eq!(debuggable_permits_test_entrypoint(" 1 \n"), Some(true));
        assert_eq!(debuggable_permits_test_entrypoint(""), None);
        assert_eq!(debuggable_permits_test_entrypoint("yes"), None);
    }

    #[test]
    fn reads_properties_out_of_a_getprop_dump() {
        let dump = "[ro.product.model]: [SM-A356B]\n[ro.debuggable]: [0]\n[ro.build.type]: [user]\n";
        assert_eq!(parse_property(dump, "ro.product.model").as_deref(), Some("SM-A356B"));
        assert_eq!(parse_property(dump, "ro.debuggable").as_deref(), Some("0"));
        assert_eq!(parse_property(dump, "ro.missing"), None);
    }

    #[test]
    fn ranks_the_dedicated_receiver_module_first() {
        let packages = vec![
            "com.google.android.cellbroadcastservice".to_string(),
            "com.samsung.android.cellbroadcastreceiver".to_string(),
            "com.android.providers.telephony".to_string(),
        ];
        let ranked = rank_cellbroadcast_packages(&packages);
        assert_eq!(
            ranked,
            vec![
                "com.samsung.android.cellbroadcastreceiver".to_string(),
                "com.google.android.cellbroadcastservice".to_string(),
            ]
        );
    }

    #[test]
    fn capability_stages_are_ordered_so_stronger_claims_compare_greater() {
        assert!(CapabilityStage::PackagePresent < CapabilityStage::ReceiverDiscovered);
        assert!(CapabilityStage::ReceiverDiscovered < CapabilityStage::TestEntryPointDiscovered);
        assert!(CapabilityStage::TestEntryPointDiscovered < CapabilityStage::TestEntryPointAccepted);
        assert!(CapabilityStage::TestEntryPointAccepted < CapabilityStage::CellBroadcastServiceReached);
        assert!(CapabilityStage::CellBroadcastServiceReached < CapabilityStage::SystemUiReached);
    }

    // -----------------------------------------------------------------------------------------
    // The enumerated native test surface
    // -----------------------------------------------------------------------------------------

    /// The enumeration must contain every alert-bearing action the receiver handles, each marked
    /// protected. Losing a row would silently narrow the claim "we checked everything".
    #[test]
    fn every_protected_alert_action_is_enumerated() {
        let actions: Vec<&str> = entry_points().iter().map(|entry| entry.action).collect();
        for action in [
            "android.provider.action.SMS_EMERGENCY_CB_RECEIVED",
            "android.provider.Telephony.SMS_CB_RECEIVED",
            "android.provider.Telephony.SMS_SERVICE_CATEGORY_PROGRAM_DATA_RECEIVED",
            "android.telephony.action.DEFAULT_SMS_SUBSCRIPTION_CHANGED",
            "android.telephony.action.CARRIER_CONFIG_CHANGED",
            "android.intent.action.SERVICE_STATE",
            "android.intent.action.LOCALE_CHANGED",
            "android.intent.action.BOOT_COMPLETED",
        ] {
            assert!(
                actions.contains(&action),
                "the protected action {action} must be enumerated"
            );
        }
    }

    /// The firmware registry must carry the exact build it was read from, so a reader can tell
    /// which phone the "PROVEN (firmware)" label refers to. A fact set with no build name would be
    /// unfalsifiable.
    #[test]
    fn the_firmware_facts_name_the_exact_build() {
        let facts = samsung_firmware_facts();
        assert!(!facts.is_empty(), "the Samsung surface must be enumerated");
        assert!(
            A35_FIRMWARE_BUILD.contains("A356BXXS4AYD1") && A35_FIRMWARE_BUILD.contains("user"),
            "the registry must name the exact retail build: {A35_FIRMWARE_BUILD}"
        );
        // Every fact must have a non-empty reference, so no claim is unattributed.
        for fact in &facts {
            assert!(
                !fact.reference.trim().is_empty(),
                "{} has no reference",
                fact.subject
            );
            assert!(!fact.claim.trim().is_empty());
        }
    }

    /// The keystone claim: on this build the AOSP test receiver's only gate is ro.debuggable=0.
    /// If this fact is dropped or relabelled, the conclusion below it becomes an opinion.
    #[test]
    fn the_firmware_registry_records_the_debuggable_zero_build() {
        let facts = samsung_firmware_facts();
        let build = facts
            .iter()
            .find(|fact| fact.subject == "Build type")
            .expect("the build type must be recorded");
        assert_eq!(build.evidence, FirmwareEvidence::ProvenOnFirmware);
        assert!(build.claim.contains("ro.debuggable=0"));
        assert!(build.claim.contains("never registered"));
    }

    /// Samsung shipping Google's module unmodified is a firmware fact, not an inference. It is the
    /// difference between "we assumed no OEM injector" and "we looked and there is none".
    #[test]
    fn the_registry_records_that_no_oem_injector_was_found() {
        let facts = samsung_firmware_facts();
        for subject in [
            "Cell Broadcast implementation",
            "Samsung RRO overlay",
            "AOSP telephony test receiver",
            "Injection-token sweep",
            "BCService",
            "SemSmsCbMessage",
        ] {
            let fact = facts
                .iter()
                .find(|fact| fact.subject == subject)
                .unwrap_or_else(|| panic!("{subject} must be enumerated"));
            assert_eq!(
                fact.evidence,
                FirmwareEvidence::ProvenOnFirmware,
                "{subject} was read from the image and must say so"
            );
        }
    }

    /// BCService must keep its corrected description. It is the single most misleading name in the
    /// Samsung image ("BC" reads as Cell Broadcast) and it is a tcpdump logger.
    #[test]
    fn bcservice_is_recorded_as_not_cell_broadcast() {
        let fact = samsung_firmware_facts()
            .into_iter()
            .find(|fact| fact.subject == "BCService")
            .expect("BCService must be enumerated");
        assert!(fact.claim.contains("not Cell Broadcast"));
        assert!(fact.claim.contains("tcpdump"));
    }

    /// The conclusion is generated from the registry, so it cannot claim more than the rows carry.
    #[test]
    fn the_conclusion_is_generated_from_the_registry() {
        let conclusion = samsung_native_entrypoint_conclusion();
        let proven = samsung_firmware_facts()
            .iter()
            .filter(|fact| fact.evidence == FirmwareEvidence::ProvenOnFirmware)
            .count();
        assert!(conclusion.contains(&proven.to_string()));
        assert!(conclusion.contains("A356BXXS4AYD1"));
        assert!(conclusion.contains("ro.debuggable=0"));
        assert!(conclusion.contains("no injector") || conclusion.contains("adds no injector"));
    }

    /// A fact that was not established from the image must never be labelled as if it were. This is
    /// the honesty guard for the whole registry.
    #[test]
    fn unproven_claims_are_never_labelled_as_firmware_proven() {
        for fact in samsung_firmware_facts() {
            if fact.evidence == FirmwareEvidence::UnprovenHere {
                assert!(
                    !fact.reference.contains("A356BXXS4AYD1") || fact.reference.contains("not present"),
                    "an unproven fact must not cite the image as its source: {}",
                    fact.subject
                );
            }
        }
        assert_eq!(FirmwareEvidence::UnprovenHere.label(), "UNPROVEN");
        assert!(FirmwareEvidence::ProvenOnFirmware.label().contains("PROVEN"));
    }

    /// The secret code must be recorded as a toggle, not an injector, on the firmware too — the
    /// AOSP lesson must survive into the Samsung-specific record.
    #[test]
    fn the_firmware_record_keeps_the_secret_code_as_a_toggle() {
        let fact = samsung_firmware_facts()
            .into_iter()
            .find(|fact| fact.subject == "Secret code 2627")
            .expect("the secret code must be recorded");
        assert!(fact.claim.contains("toggles"));
        assert!(fact.claim.contains("constructs no"));
        assert!(fact.claim.contains("protected broadcast"));
    }

    /// An AOSP-derived row must not be counted among the firmware-proven rows, or the "N of M
    /// proven from the image" number would overstate what was actually read from the phone.
    #[test]
    fn aosp_proven_rows_are_not_counted_as_firmware_proven() {
        let facts = samsung_firmware_facts();
        let aosp = facts
            .iter()
            .filter(|fact| fact.evidence == FirmwareEvidence::ProvenOnAosp)
            .count();
        let firmware = facts
            .iter()
            .filter(|fact| fact.evidence == FirmwareEvidence::ProvenOnFirmware)
            .count();
        assert!(aosp >= 1, "the AOSP yardstick row must exist");
        assert!(firmware >= 1);
        assert!(aosp + firmware <= facts.len());
        assert_eq!(FirmwareEvidence::ProvenOnAosp.label(), "PROVEN (AOSP)");
    }

    /// The secret code must be classified as protected *and* not counted as a way in. It is a
    /// gate-opener, so treating it as reachable would overclaim the whole project.
    #[test]
    fn the_secret_code_is_protected_and_not_a_route_in() {
        let secret = entry_points()
            .into_iter()
            .find(|entry| entry.action.contains("SECRET_CODE"))
            .expect("the 2627 secret code must be enumerated");
        assert_eq!(secret.outcome, EntryPointOutcome::ProtectedBroadcast);
        assert!(
            !reachable_entry_points().iter().any(|entry| entry.action.contains("SECRET_CODE")),
            "the secret code only toggles a display filter; it must never be reported reachable"
        );
    }

    /// Exactly the three telephony test actions are reachable, and nothing else. If a future edit
    /// adds a fourth, this test fails and forces the reason to be stated.
    #[test]
    fn only_the_telephony_test_receivers_are_reachable() {
        let reachable = reachable_entry_points();
        assert_eq!(
            reachable.len(),
            3,
            "expected exactly three reachable entry points, found {:?}",
            reachable.iter().map(|entry| entry.action).collect::<Vec<_>>()
        );
        assert!(reachable.iter().all(|entry| entry.action.contains("TEST_TRIGGER")));
        assert!(reachable.iter().any(|entry| entry.action == TEST_TRIGGER_ACTION));
    }

    /// The summary sentence is derived from the enumeration, so its counts must agree with it.
    #[test]
    fn the_summary_counts_match_the_enumeration() {
        let all = entry_points().len();
        let reachable = reachable_entry_points().len();
        assert!(all > reachable, "most entry points must not be reachable");
        let summary = entry_point_summary();
        assert!(summary.contains(&format!("{all} entry points were enumerated")));
        assert!(summary.contains(&format!("{reachable} are reachable")));
        assert!(summary.contains("ro.debuggable=1"));
    }

    /// Every outcome must carry an explanation, so no row is a bare label.
    #[test]
    fn every_outcome_explains_itself() {
        for outcome in [
            EntryPointOutcome::Reachable,
            EntryPointOutcome::ProtectedBroadcast,
            EntryPointOutcome::NotExported,
            EntryPointOutcome::NotAnInjector,
            EntryPointOutcome::SignatureOrBuildGate,
        ] {
            assert!(!outcome.explain().is_empty());
            assert!(!outcome.label().is_empty());
        }
    }

    // -----------------------------------------------------------------------------------------
    // The alert-channel catalogue
    // -----------------------------------------------------------------------------------------

    /// Every catalogue entry must be usable: a stable identifier, a label that carries it so a log
    /// line can be matched back to this table, and a stated gate and requirement.
    #[test]
    fn every_channel_is_self_describing() {
        for channel in ALERT_CHANNELS {
            assert!(!channel.label.is_empty());
            assert!(!channel.effect.is_empty());
            assert!(!channel.gate.is_empty());
            assert!(!channel.requirement.is_empty());
            assert!(
                channel.label.contains(&format!("0x{:04X}", channel.message_id)),
                "channel label {:?} must name its identifier",
                channel.label
            );
        }
    }

    /// A disabled-by-default channel must say what to enable, and an enabled-by-default channel must
    /// not. This is the distinction the catalogue exists for: "it will not appear" has to come with
    /// "and here is why", and "it will appear" must not invent a prerequisite the operator would
    /// then go and satisfy for nothing.
    #[test]
    fn the_requirement_text_matches_the_default_state() {
        for channel in ALERT_CHANNELS {
            let claims_nothing_needed = channel.requirement.starts_with("Nothing");
            assert_eq!(
                claims_nothing_needed, channel.enabled_by_default,
                "channel 0x{:04X} says enabled_by_default={} but its requirement is {:?}",
                channel.message_id, channel.enabled_by_default, channel.requirement
            );
        }
    }

    /// Identifiers must be unique, or a lookup would silently pick whichever came first.
    #[test]
    fn channel_identifiers_are_unique() {
        let mut ids: Vec<u16> = ALERT_CHANNELS.iter().map(|c| c.message_id).collect();
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(before, ids.len(), "two channels share a message identifier");
    }

    /// The default channel must be one that works without the operator changing anything, and the
    /// ETWS test channel must not be it — that inversion is the defect this catalogue repairs.
    #[test]
    fn the_default_channel_needs_no_operator_action() {
        let default = default_alert_channel();
        assert!(
            default.enabled_by_default,
            "the default channel must be on for an unconfigured device"
        );
        assert_ne!(
            default.message_id, MESSAGE_ID_ETWS_TEST,
            "0x1103 is disabled unless testing mode is on; it must not be the default"
        );
    }

    /// `isChannelEnabled` in `CellBroadcastAlertService` gates these four channels differently, and
    /// the differences are recorded here so a future edit cannot quietly lose one.
    #[test]
    fn the_catalogue_records_the_as_p_is_channel_enabled_gates() {
        let etws = alert_channel(MESSAGE_ID_ETWS_EARTHQUAKE).expect("0x1100 is catalogued");
        assert!(etws.enabled_by_default);
        assert_eq!(etws.gate, "master toggle only");

        let extreme = alert_channel(0x1113).expect("0x1113 is catalogued");
        assert!(extreme.enabled_by_default);
        assert!(extreme.gate.contains("EXTREME"));

        let monthly = alert_channel(0x111C).expect("0x111C is catalogued");
        assert!(!monthly.enabled_by_default);
        assert!(monthly.gate.contains("TEST_ALERTS"));

        let etws_test = alert_channel(MESSAGE_ID_ETWS_TEST).expect("0x1103 is catalogued");
        assert!(!etws_test.enabled_by_default);
        assert!(
            etws_test.gate.contains("testing mode"),
            "0x1103 additionally requires testing mode; the gate text must say so"
        );
    }

    /// An unregistered identifier must be refused rather than encoded, so a PDU can never be built
    /// for a channel whose receive-side gate nobody has checked.
    #[test]
    fn an_uncatalogued_channel_cannot_be_encoded() {
        let err = cb_pdu(0x9999, "TEST whatever", 0).expect_err("0x9999 is not catalogued");
        assert!(err.contains("0x9999"), "the error must name the identifier: {err}");
    }

    /// The channel identifier must land in the header where `SmsCbHeader` reads the message
    /// identifier, and the body must still round-trip.
    #[test]
    fn a_non_default_channel_encodes_into_the_header() {
        for channel in ALERT_CHANNELS {
            let body = "TEST CHANNEL ROUTING";
            let pdu = cb_pdu(channel.message_id, body, 0x0042)
                .unwrap_or_else(|e| panic!("0x{:04X} must encode: {e}", channel.message_id));
            let header = parse_cb_header(&pdu).expect("built pdu has a header");
            assert_eq!(header.message_id, channel.message_id);
            assert_eq!(header.serial_number, 0x0042);
            assert_eq!(header.data_coding_scheme, DCS_GSM7);
            assert_eq!(decode_body(&pdu, body.len()), body);
        }
    }

    /// The reconstructed PDU for the ETWS test channel must be byte-identical to the previous
    /// single-channel builder, so the refactor is provably behaviour-preserving for that channel.
    #[test]
    fn the_refactor_preserved_the_etws_test_encoding() {
        let body = "TEST ALERT - SIMULATION";
        let via_legacy = etws_test_pdu(body, 0x1234).expect("legacy path encodes");
        let via_catalogue = cb_pdu(MESSAGE_ID_ETWS_TEST, body, 0x1234).expect("catalogue encodes");
        assert_eq!(via_legacy, via_catalogue);
        assert_eq!(&via_catalogue[..6], &[0x12, 0x34, 0x11, 0x03, 0x11, 0x01]);
    }

    // -----------------------------------------------------------------------------------------
    // PDU construction
    // -----------------------------------------------------------------------------------------

    /// The header parser must agree with the layout AOSP itself documents.
    #[test]
    fn parses_the_aosp_reference_pdu_header() {
        let pdu = hex_decode(AOSP_REFERENCE_PDU_HEX).expect("reference vector is valid hex");
        let header = parse_cb_header(&pdu).expect("reference vector has a full header");
        assert_eq!(header.serial_number, 0x0000);
        assert_eq!(header.message_id, MESSAGE_ID_ETWS_EARTHQUAKE);
        assert_eq!(header.data_coding_scheme, DCS_GSM7);
        assert_eq!(header.page_parameter, 0x01);
        assert_eq!(header.message_id_label(), "ETWS EARTHQUAKE WARNING (0x1100)");
    }

    #[test]
    fn builds_a_pdu_with_the_expected_header() {
        let pdu = etws_test_pdu("TEST ALERT", 0x1234).expect("plain ASCII encodes");
        let header = parse_cb_header(&pdu).expect("built pdu has a header");
        assert_eq!(header.serial_number, 0x1234);
        assert_eq!(header.message_id, MESSAGE_ID_ETWS_TEST);
        assert_eq!(header.data_coding_scheme, DCS_GSM7);
        assert_eq!(header.page_parameter, 0x01);
        assert_eq!(&pdu[..6], &[0x12, 0x34, 0x11, 0x03, 0x11, 0x01]);
    }

    /// A PDU that carries a body must round-trip that body back through the unpacker.
    #[test]
    fn the_body_round_trips_through_the_packing() {
        let body = "TEST ALERT - SIMULATION";
        let pdu = etws_test_pdu(body, 0).expect("body encodes");
        assert_eq!(decode_body(&pdu, body.len()), body);
    }

    /// The number of septets must be exactly the character count, with the eighth bit unused.
    #[test]
    fn packing_a_septet_vector_is_bit_exact() {
        // "TEST" -> T=0x54, E=0x45, S=0x53, T=0x54 (all under 0x80 so ASCII == septet)
        let packed = pack_septets(&[0x54, 0x45, 0x53, 0x54]);
        assert_eq!(packed, vec![0xD4, 0xE2, 0x94, 0x0A]);
        assert_eq!(unpack_septets(&packed, 4), vec![0x54, 0x45, 0x53, 0x54]);
    }

    #[test]
    fn round_trips_every_printable_ascii_septet() {
        let septets: Vec<u8> = (0x20u8..0x7F).collect();
        assert_eq!(unpack_septets(&pack_septets(&septets), septets.len()), septets);
    }

    /// Without a supplied count the unpacker must not invent trailing characters from padding bits.
    #[test]
    fn unpacking_honours_the_septet_count() {
        let packed = pack_septets(&[0x54, 0x45, 0x53, 0x54]);
        assert_eq!(unpack_septets(&packed, 4).len(), 4);
        assert_eq!(unpack_septets(&packed, 2), vec![0x54, 0x45]);
    }

    /// Characters needing the extension table must be refused, not silently dropped.
    #[test]
    fn refuses_characters_outside_the_basic_alphabet() {
        assert!(encode_gsm7("TEST ✓").is_err());
        assert!(encode_gsm7("TEST €").is_err());
        assert!(gsm7_septet('\u{1b}').is_none());
    }

    /// A body that is not a test message must never be packed.
    #[test]
    fn refuses_a_body_that_is_not_a_test_message() {
        assert!(etws_test_pdu("EARTHQUAKE WARNING", 0).is_err());
        assert!(etws_test_pdu("", 0).is_err());
    }

    #[test]
    fn refuses_a_body_that_exceeds_one_page() {
        let body = format!("TEST {}", "A".repeat(96));
        assert!(etws_test_pdu(&body, 0).is_err());
        let fits = format!("TEST {}", "A".repeat(88));
        assert!(etws_test_pdu(&fits, 0).is_ok());
    }

    /// Shell metacharacters that are also basic-alphabet characters must survive unchanged, because
    /// this value is what reaches the device.
    #[test]
    fn a_hostile_body_survives_encoding() {
        let body = "TEST \"quoted\" 'single' $(id) ; rm -rf / && echo < > $HOME ; #";
        let pdu = etws_test_pdu(body, 7).expect("these metacharacters are basic alphabet");
        assert_eq!(decode_body(&pdu, body.len()), body);
    }

    /// Characters outside the GSM 7-bit default alphabet must be refused explicitly.
    ///
    /// The extension-table characters (`|`, `{`, `}`, `\`, backtick, the euro sign) and characters
    /// in no GSM table at all (`°`) are all outside the basic alphabet. Refusing them is the honest
    /// outcome: the alternative is an alert whose on-device text silently differs from the text the
    /// operator typed. This is a real constraint on alert bodies, not an implementation artefact.
    #[test]
    fn characters_outside_the_alphabet_are_refused_by_name() {
        for character in ['`', '\u{20ac}', '\\', '|', '{', '}', '~', '[', ']', '\u{b0}', '€'] {
            let body = format!("TEST {character}");
            let error = etws_test_pdu(&body, 0).expect_err("must refuse");
            assert!(
                error.contains("GSM 7-bit default alphabet"),
                "error must name the alphabet: {error}"
            );
        }
    }

    /// The characters the basic table does define must encode, including the non-obvious ones.
    #[test]
    fn the_basic_alphabet_encodes_including_its_oddities() {
        assert!(etws_test_pdu("TEST line\nbreak", 0).is_ok());
        assert!(etws_test_pdu("TEST \n\r@pound:\u{a3} yen:\u{a5} dollar:$", 0).is_ok());
        assert!(etws_test_pdu("TEST \u{c4}\u{d6}\u{dc} \u{e4}\u{f6}\u{fc}\u{df}", 0).is_ok());
        assert!(etws_test_pdu("TEST \u{394}\u{3a9}\u{39e}", 0).is_ok());
        assert!(etws_test_pdu("TEST a\u{a4}b", 0).is_ok());
    }

    /// The device-side shell is not the only layer: `--es pdu_string <hex>` is re-split by the
    /// device shell, so the hex itself must contain nothing a shell can act on.
    #[test]
    fn the_pdu_hex_is_shell_safe() {
        let pdu = etws_test_pdu("TEST", 0).unwrap();
        let hex = hex_encode(&pdu);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(hex, hex.to_ascii_uppercase());
    }

    /// The body the operator typed is the body the device decodes, bit for bit.
    ///
    /// The expected length is passed in rather than inferred: the padding bits in the final octet
    /// are indistinguishable from real septets, so a decoder cannot know where the message ends
    /// without being told. That is a real property of the format, not a limitation of this helper.
    fn decode_body(pdu: &[u8], expected_len: usize) -> String {
        unpack_septets(&pdu[6..], expected_len)
            .iter()
            .map(|&s| GSM7_BASIC.chars().nth(s as usize).unwrap_or('\u{fffd}'))
            .collect()
    }

    fn hex_encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02X}")).collect()
    }

    /// There must be no way to produce a non-test message identifier.
    #[test]
    fn the_message_identifier_is_fixed_to_the_test_channel() {
        for serial in [0u16, 1, 0xFFFF] {
            let pdu = etws_test_pdu("TEST", serial).unwrap();
            assert_eq!(parse_cb_header(&pdu).unwrap().message_id, MESSAGE_ID_ETWS_TEST);
            assert_eq!(ETWS_TEST_SERVICE_CATEGORY, 4355);
        }
    }

    fn hex_decode(hex: &str) -> Option<Vec<u8>> {
        if hex.len() % 2 != 0 {
            return None;
        }
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
            .collect()
    }

    // -----------------------------------------------------------------------------------------
    // Platform test path: the boundary the project kept blurring
    // -----------------------------------------------------------------------------------------

    #[test]
    fn a_retail_build_is_reported_as_unavailable_rather_than_supported() {
        let entry = assess_test_entrypoint("0", Some("com.google.android.cellbroadcastreceiver"));
        assert_eq!(entry.available, State::Denied);
        assert!(entry.reason.contains("ro.debuggable=0"));
        assert!(entry.reason.contains("cannot be granted"));
    }

    #[test]
    fn a_userdebug_build_is_reported_as_available() {
        let entry = assess_test_entrypoint("1", Some("com.google.android.cellbroadcastreceiver"));
        assert_eq!(entry.available, State::Granted);
    }

    #[test]
    fn an_unreadable_property_is_unknown_not_denied() {
        let entry = assess_test_entrypoint("", Some("com.google.android.cellbroadcastreceiver"));
        assert_eq!(entry.available, State::Unknown);
    }

    #[test]
    fn a_device_with_no_receiver_is_not_assumed_capable() {
        assert_eq!(assess_test_entrypoint("1", None).available, State::NotPresent);
    }

    /// An accepted command with no downstream lines must not be called a delivery.
    #[test]
    fn accepted_by_am_but_nothing_downstream_proves_nothing() {
        let evidence = scan_platform_logcat("Broadcast completed: result=0\n");
        assert!(!evidence.pipeline_ran());
        assert_eq!(evidence.stage(), CapabilityStage::TestEntryPointDiscovered);
    }

    #[test]
    fn each_downstream_marker_advances_the_stage_by_one_claim() {
        let receiver = "I GsmInboundSmsHandler: Received test intent action=com.android.internal.telephony.gsm.TEST_TRIGGER_CELL_BROADCAST\n";
        assert_eq!(
            scan_platform_logcat(receiver).stage(),
            CapabilityStage::TestEntryPointAccepted
        );
        assert!(!scan_platform_logcat(receiver).service_reached);

        let service = format!("{receiver}I CellBroadcastHandler: handleGsmCellBroadcastSms\n");
        assert_eq!(
            scan_platform_logcat(&service).stage(),
            CapabilityStage::CellBroadcastServiceReached
        );

        let alert_service = format!("{service}I CBAlertService: onStartCommand: {action}\n", action="android.provider.Telephony.SMS_CB_RECEIVED");
        // The alert service starting is *not* the UI. It fires before the channel-range and
        // testing-mode gates, so it is its own rung and it must not claim presentation.
        assert_eq!(
            scan_platform_logcat(&alert_service).stage(),
            CapabilityStage::AlertServiceReached
        );

        let ui = format!("{alert_service}D CBAlertService: openEmergencyAlertNotification\n");
        assert_eq!(scan_platform_logcat(&ui).stage(), CapabilityStage::SystemUiReached);
    }

    /// Package presence alone must never exceed the weakest stage.
    #[test]
    fn package_presence_is_not_capability() {
        let evidence = PlatformEvidence::default();
        assert_eq!(evidence.stage(), CapabilityStage::TestEntryPointDiscovered);
        assert!(!evidence.pipeline_ran());
    }

    /// A stray broadcast the Cell Broadcast app rejects must not be read as the alert path running.
    ///
    /// The app's receiver tag is `CellBroadcastReceiver` and its `onReceive` logs an "unexpected
    /// action" warning for anything it does not handle. Matching the bare tag would have turned a
    /// rejected broadcast into a `SYSTEM UI REACHED` claim.
    #[test]
    fn a_rejected_broadcast_to_the_receiver_tag_is_not_a_delivery() {
        let rejected = "D CellBroadcastReceiver: onReceive Intent { act=com.example.SOMETHING_ELSE }\n\
W CellBroadcastReceiver: onReceive() unexpected action com.example.SOMETHING_ELSE\n";
        let evidence = scan_platform_logcat(rejected);
        assert!(!evidence.receiver_processed);
        assert!(!evidence.pipeline_ran());
        assert_eq!(evidence.stage(), CapabilityStage::TestEntryPointDiscovered);
    }

    /// The paths the app does handle are evidence that it processed the message.
    #[test]
    fn the_handled_cb_actions_are_recognised_as_processing() {
        for action in [
            "android.provider.Telephony.SMS_CB_RECEIVED",
            "android.provider.action.SMS_EMERGENCY_CB_RECEIVED",
        ] {
            let line = format!("D CellBroadcastReceiver: onReceive Intent {{ act={action} }}\n");
            assert!(
                scan_platform_logcat(&line).receiver_processed,
                "{action} must count as processing"
            );
        }
    }

    /// A message the platform deliberately dropped is a definite result with a stated cause.
    ///
    /// This is the difference between "nothing happened and we do not know why" and "the OEM master
    /// switch is on". Reporting `UNKNOWN` here would hide the most explicit thing in the capture.
    #[test]
    fn an_oem_disabled_message_is_reported_as_suppressed_not_unknown() {
        let log = "D CellBroadcastServiceManager: GSM CB message ignored - CB messages disabled by OEM.\n";
        let evidence = scan_platform_logcat(log);
        assert!(evidence.was_suppressed());
        assert_eq!(
            evidence.suppression.as_ref().unwrap().gate,
            SuppressionGate::DisabledByOem
        );
        let explanation = evidence.suppression.as_ref().unwrap().explain();
        assert!(explanation.contains("config_disable_all_cb_messages"));
        assert!(explanation.contains("not a permission"));
    }

    /// Each suppression gate is distinguished, because each implies a different next step.
    #[test]
    fn each_suppression_gate_is_identified_separately() {
        let cases = [
            (
                "ignoring the alert due to not in testing mode",
                SuppressionGate::TestModeRequired,
            ),
            (
                "ignoring the alert due to configured channels was marked",
                SuppressionGate::ChannelDisabled,
            ),
            (
                "ignoring the alert due to language mismatch",
                SuppressionGate::LanguageMismatch,
            ),
            ("Skipped message due to filter: foo", SuppressionGate::ContentFilter),
        ];
        for (line, expected) in cases {
            let evidence = scan_platform_logcat(line);
            assert_eq!(
                evidence.suppression.as_ref().map(|s| s.gate),
                Some(expected),
                "line {line:?} must be identified"
            );
        }
    }

    /// The raw device text is preserved so a report can quote the device, not paraphrase it.
    #[test]
    fn the_suppression_keeps_the_verbatim_device_line() {
        let line = "GSM CB message ignored - CB messages disabled by OEM.";
        let evidence = scan_platform_logcat(line);
        let sent = evidence.suppression.unwrap().sent;
        assert!(
            line.contains(&sent),
            "the stored text {sent:?} must come from the device line {line:?}"
        );
    }

    /// A capture with no suppression marker must not invent one.
    #[test]
    fn a_normal_capture_reports_no_suppression() {
        let log = "I CBAlertService: onStartCommand\n";
        assert!(!scan_platform_logcat(log).was_suppressed());
    }

    /// A capture can contain both positive traces and a stated drop, and the drop must win.
    ///
    /// `CBAlertService: onStartCommand` fires before the testing-mode and channel-range checks, so a
    /// real capture of a gated message looks like this: the service started *and* the platform said
    /// it dropped the alert. `pipeline_ran()` is true and `was_suppressed()` is true at once. The
    /// caller must consult `was_suppressed()` first; this test pins that both are observable so the
    /// ordering in the verdict is a decision rather than an accident.
    #[test]
    fn a_gated_message_shows_both_a_positive_trace_and_a_drop() {
        let log = "\
D CBAlertService: onStartCommand: android.provider.Telephony.SMS_CB_RECEIVED
D CBAlertService: ignoring the alert due to not in testing mode
";
        let evidence = scan_platform_logcat(log);
        assert!(evidence.pipeline_ran(), "the service start line is a positive trace");
        assert!(evidence.was_suppressed(), "the drop line must be visible");
        assert_eq!(
            evidence.suppression.as_ref().unwrap().gate,
            SuppressionGate::TestModeRequired
        );
    }

    /// `is_definite` must not treat a failed probe as a fact about the device.
    #[test]
    fn unknown_and_error_are_not_definite_answers() {
        assert!(State::Granted.is_definite());
        assert!(State::Denied.is_definite());
        assert!(State::NotPresent.is_definite());
        assert!(State::NotApplicable.is_definite());
        assert!(!State::Unknown.is_definite());
        assert!(!State::Error.is_definite());
        assert_eq!(State::NotApplicable.label(), "NOT_APPLICABLE");
    }

    #[test]
    fn stage_labels_match_the_ui_contract() {
        assert_eq!(CapabilityStage::PackagePresent.label(), "PACKAGE PRESENT");
        assert_eq!(CapabilityStage::TestEntryPointDiscovered.label(), "TEST ENTRY POINT DISCOVERED");
        assert_eq!(CapabilityStage::SystemUiReached.label(), "NATIVE ALERT PRESENTED");
        assert_eq!(CapabilityStage::AlertServiceReached.label(), "ALERT SERVICE REACHED");
        assert_eq!(CapabilityStage::ReceiverProcessed.label(), "CB RECEIVER PROCESSED");
        assert_eq!(State::Granted.label(), "GRANTED");
        assert_eq!(State::Unknown.label(), "UNKNOWN");
    }

    // -------------------------------------------------------------------------------------
    // Mode selection: keeping local simulation and Cell Broadcast apart
    // -------------------------------------------------------------------------------------

    /// The defect being prevented: a device with the local simulator installed is a device on which
    /// an app notification can be posted. It says nothing about Android's Cell Broadcast stack.
    #[test]
    fn the_local_simulator_is_not_a_cell_broadcast_mode() {
        let mode = alert_mode(State::Denied, true, false);
        assert_eq!(mode, AlertMode::LocalUiSimulation);
        assert!(
            !mode.is_cell_broadcast(),
            "a local app notification must never be labelled Cell Broadcast"
        );
    }

    #[test]
    fn the_platform_test_mode_is_a_cell_broadcast_mode() {
        let mode = alert_mode(State::Granted, false, false);
        assert_eq!(mode, AlertMode::PlatformCellBroadcastTest);
        assert!(mode.is_cell_broadcast());
    }

    /// The platform path wins when it exists: it is the only one that reaches Android's stack.
    #[test]
    fn the_platform_path_is_preferred_over_the_local_simulator() {
        assert_eq!(
            alert_mode(State::Granted, true, false),
            AlertMode::PlatformCellBroadcastTest
        );
    }

    /// Verification cannot be asserted into existence. A caller with no logcat evidence passes
    /// false, and there is no other way to reach the verified mode.
    #[test]
    fn verification_requires_evidence_and_is_never_implied() {
        assert_ne!(alert_mode(State::Granted, true, false), AlertMode::GenuineCellBroadcastVerified);
        assert_ne!(alert_mode(State::Denied, true, false), AlertMode::GenuineCellBroadcastVerified);
        assert_eq!(
            alert_mode(State::Granted, true, true),
            AlertMode::GenuineCellBroadcastVerified
        );
    }

    /// An unreadable entry point is unknown, and unknown is not granted. With nothing installed the
    /// honest mode is Unavailable, not a hopeful simulation.
    #[test]
    fn an_unknown_entry_point_with_nothing_installed_is_unavailable() {
        assert_eq!(alert_mode(State::Unknown, false, false), AlertMode::Unavailable);
        assert_eq!(alert_mode(State::Error, false, false), AlertMode::Unavailable);
        assert_eq!(alert_mode(State::NotPresent, false, false), AlertMode::Unavailable);
    }

    /// The mislabelling that was found in the devices list: installing the local simulator promoted
    /// the device to TestEntryPointDiscovered, which is a platform Cell Broadcast claim.
    #[test]
    fn a_local_simulator_does_not_promote_the_capability_stage() {
        // Receiver declared, but the build cannot run the test receiver.
        let stage = capability_stage(true, true, State::Denied);
        assert_eq!(stage, CapabilityStage::ReceiverDiscovered);
        assert_ne!(stage, CapabilityStage::TestEntryPointDiscovered);
    }

    #[test]
    fn the_stage_ladder_advances_only_on_real_evidence() {
        assert_eq!(capability_stage(false, false, State::Unknown), CapabilityStage::None);
        assert_eq!(capability_stage(false, true, State::Denied), CapabilityStage::PackagePresent);
        assert_eq!(capability_stage(true, true, State::Denied), CapabilityStage::ReceiverDiscovered);
        assert_eq!(
            capability_stage(true, true, State::Granted),
            CapabilityStage::TestEntryPointDiscovered
        );
        // A granted entry point with no receiver declaration is not a receiver claim.
        assert_eq!(capability_stage(false, true, State::Granted), CapabilityStage::PackagePresent);
    }

    #[test]
    fn the_mode_labels_cannot_be_confused_with_each_other() {
        assert_eq!(AlertMode::LocalUiSimulation.label(), "LOCAL UI SIMULATION");
        assert_eq!(AlertMode::Unavailable.label(), "UNAVAILABLE");
        assert!(AlertMode::GenuineCellBroadcastVerified.label().contains("VERIFIED"));
    }

    // -----------------------------------------------------------------------------------------
    // The non-broadcast interface survey
    // -----------------------------------------------------------------------------------------

    /// The whole point of the survey is that the classes were actually covered, so the enumeration
    /// must contain more than the broadcast layer: shell commands, Binder methods and OEM
    /// components all have to be present.
    #[test]
    fn the_interface_survey_covers_every_class() {
        let classes: Vec<InterfaceClass> =
            interface_candidates().into_iter().map(|c| c.class).collect();
        for expected in [
            InterfaceClass::ShellCommand,
            InterfaceClass::BinderService,
            InterfaceClass::TelephonyBinder,
            InterfaceClass::OemComponent,
            InterfaceClass::StateOnly,
        ] {
            assert!(
                classes.contains(&expected),
                "the survey must include {expected:?}, or a whole interface class was skipped"
            );
        }
    }

    /// The interfaces that can actually feed the pipeline must be exactly the ICellBroadcastService
    /// handlers, and they must be recorded as property-gated -- not reachable, and not
    /// signature-gated. Getting this wrong either over- or under-claims the product.
    #[test]
    fn the_only_property_gated_injectors_are_the_cb_service_handlers() {
        let gated = property_gated_injectors();
        assert!(!gated.is_empty());
        for candidate in &gated {
            assert!(
                candidate.interface.contains("ICellBroadcastService")
                    || candidate.interface.contains("handle"),
                "unexpected property-gated injector: {}",
                candidate.interface
            );
            assert_eq!(candidate.outcome, InterfaceOutcome::BuildPropertyGate);
        }
    }

    /// `cmd cellbroadcast` must be recorded as absent, because a reader who only sees "some shell
    /// commands exist" would reasonably assume a Cell Broadcast command exists too. It does not.
    #[test]
    fn the_cellbroadcast_shell_command_is_recorded_as_absent() {
        let missing = interface_candidates()
            .into_iter()
            .find(|c| c.interface.starts_with("cmd cellbroadcast"))
            .expect("the absent `cmd cellbroadcast` must be enumerated");
        assert_eq!(missing.outcome, InterfaceOutcome::Absent);
    }

    /// The MockModem route is the one that *would* replay broadcast SMS through the real radio
    /// callback, so it must be present in the survey and marked unavailable, with the image's lack
    /// of the package as the reason. Losing this row would lose the strongest near-miss.
    #[test]
    fn the_mock_modem_route_is_recorded_with_its_reason() {
        let mock = interface_candidates()
            .into_iter()
            .find(|c| c.interface.contains("set-modem-service"))
            .expect("the MockModem route must be enumerated");
        assert_eq!(mock.outcome, InterfaceOutcome::Absent);
        assert!(mock.note.contains("MODIFY_PHONE_STATE"));
        assert!(mock.note.contains("not present"));
    }

    /// The Samsung CMAS database rows are real and they are *not* an injector. This is the row most
    /// likely to be mistaken for one, so it is pinned.
    #[test]
    fn the_samsung_cmas_rows_are_not_an_injector() {
        let cmas = interface_candidates()
            .into_iter()
            .find(|c| c.interface.contains("SecTelephonyProvider"))
            .expect("the Samsung CMAS row must be enumerated");
        assert_eq!(cmas.outcome, InterfaceOutcome::ConfigOnly);
    }

    #[test]
    fn the_interface_summary_is_generated_from_the_rows() {
        let summary = interface_summary();
        let all = interface_candidates().len();
        assert!(summary.contains(&format!("{all} non-broadcast interfaces")));
        assert!(summary.contains("ro.debuggable"));
    }

    // -----------------------------------------------------------------------------------------
    // The verification ladder
    // -----------------------------------------------------------------------------------------

    /// The ladder must be ordered from connection to success and it must be strictly longer than a
    /// trivially correct `if exit_code == 0` check -- i.e. it must name the pipeline stages.
    #[test]
    fn the_verification_ladder_is_ordered_and_complete() {
        let ladder = verification_ladder();
        assert_eq!(ladder.first().unwrap().stage, VerificationStage::DeviceConnected);
        assert_eq!(ladder.last().unwrap().stage, VerificationStage::Success);
        assert!(ladder.len() >= 10);
        assert!(ladder.iter().any(|r| r.stage == VerificationStage::TriggerSent));
        assert!(ladder.iter().any(|r| r.stage == VerificationStage::NativeAlertDetected));
    }

    /// The ladder's success rung must require the native-alert evidence, and nothing weaker. This is
    /// the assertion that stops the project regressing to "am exited 0".
    #[test]
    fn success_requires_the_native_alert_and_nothing_weaker() {
        let ladder = verification_ladder();
        let success = ladder
            .iter()
            .find(|r| r.stage == VerificationStage::Success)
            .unwrap();
        assert!(success.requires.contains("NATIVE_ALERT_DETECTED"));
        assert!(success.requires.contains("no suppression"));
    }

    /// An empty capture after a send proves only that the trigger was sent. It must never be read as
    /// a pipeline stage, let alone success.
    #[test]
    fn an_empty_capture_after_a_send_is_only_trigger_sent() {
        let evidence = PlatformEvidence::default();
        assert_eq!(evidence.verification_stage(), VerificationStage::TriggerSent);
        assert_eq!(evidence.stage(), CapabilityStage::TestEntryPointDiscovered);
    }

    /// A suppressed run reaches the alert service and stops: it must never map to success, and it
    /// must report the alert-service rung rather than the UI rung.
    #[test]
    fn a_suppressed_run_can_never_be_success() {
        let evidence = PlatformEvidence {
            alert_service_started: true,
            suppression: Some(Suppression {
                gate: SuppressionGate::ChannelDisabled,
                sent: "ignoring the alert due to configured channels was marked".to_string(),
            }),
            ..Default::default()
        };
        assert_ne!(evidence.verification_stage(), VerificationStage::Success);
        assert_eq!(evidence.verification_stage(), VerificationStage::AlertServiceDetected);
        assert_eq!(evidence.stage(), CapabilityStage::AlertServiceReached);
        assert!(evidence.was_suppressed());
    }

    /// The alert service starting is *not* the UI being presented. This is the exact
    /// conflation the marker split exists to prevent.
    #[test]
    fn the_alert_service_starting_is_not_the_ui() {
        let evidence = PlatformEvidence {
            alert_service_started: true,
            ..Default::default()
        };
        assert_eq!(evidence.stage(), CapabilityStage::AlertServiceReached);
        assert_eq!(evidence.verification_stage(), VerificationStage::AlertServiceDetected);
        assert_ne!(evidence.verification_stage(), VerificationStage::Success);
    }

    /// Only a real presentation marker reaches the top rung.
    #[test]
    fn the_native_presentation_marker_reaches_success() {
        let evidence = PlatformEvidence {
            alert_service_started: true,
            alert_ui_requested: true,
            ..Default::default()
        };
        assert_eq!(evidence.stage(), CapabilityStage::SystemUiReached);
        assert_eq!(evidence.verification_stage(), VerificationStage::Success);
    }

    /// The suppression strings are the exact AOSP text. If an upstream change rewrites them, this
    /// fails loudly rather than silently turning a suppressed run into an unexplained one.
    #[test]
    fn the_suppression_markers_match_the_aosp_source_text() {
        for (marker, _gate) in SUPPRESSION_MARKERS {
            assert!(!marker.is_empty());
        }
        let capture = "07-01 CBAlertService: ignoring the alert due to not in testing mode";
        let evidence = scan_platform_logcat(capture);
        assert_eq!(
            evidence.suppression.as_ref().unwrap().gate,
            SuppressionGate::TestModeRequired
        );
    }
}
