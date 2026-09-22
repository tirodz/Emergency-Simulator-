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
    /// Downstream evidence shows Android's own alert UI was presented.
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
            CapabilityStage::SystemUiReached => "SYSTEM UI REACHED",
        }
    }
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
    if !body.starts_with("TEST") {
        return Err("refusing to build an alert whose body does not begin with TEST".to_string());
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
    pdu.extend_from_slice(&MESSAGE_ID_ETWS_TEST.to_be_bytes());
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
    pub fn stage(&self) -> CapabilityStage {
        if self.alert_ui_requested || self.receiver_processed {
            CapabilityStage::SystemUiReached
        } else if self.service_reached {
            CapabilityStage::CellBroadcastServiceReached
        } else if self.test_receiver_accepted {
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
/// process existed. The bare class tag is deliberately absent; see the comment above.
const MARKER_RECEIVER: &[&str] = &[
    "CellBroadcastReceiver: onReceive android.provider.Telephony.SMS_CB_RECEIVED",
    "CellBroadcastReceiver: onReceive android.provider.action.SMS_EMERGENCY_CB_RECEIVED",
];
const MARKER_ALERT_UI: &[&str] = &[
    "CBAlertService: onStartCommand",
    "CellBroadcastAlertDialog",
    "openEmergencyAlertNotification",
];

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

        let ui = format!("{service}I CellBroadcastAlertDialog: onCreate\n");
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
        let rejected = "W CellBroadcastReceiver: onReceive() unexpected action com.example.SOMETHING_ELSE\n";
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
            let line = format!("D CellBroadcastReceiver: onReceive {action}\n");
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
        let log = "I CellBroadcastAlertDialog: onCreate\n";
        assert!(!scan_platform_logcat(log).was_suppressed());
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
        assert_eq!(CapabilityStage::SystemUiReached.label(), "SYSTEM UI REACHED");
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
}
