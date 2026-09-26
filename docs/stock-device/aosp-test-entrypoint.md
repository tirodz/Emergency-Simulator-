# The AOSP test entry point: what it is, and the one property that gates it

**Status:** CONFIRMED against primary AOSP source, fetched during this session.
**Device relevance:** This is the mechanism the project's platform test path depends on. It decides
everything about what a stock Galaxy A35 can and cannot do.

---

## The mechanism

`frameworks/opt/telephony/src/java/com/android/internal/telephony/gsm/GsmInboundSmsHandler.java`
constructs a private receiver whose only purpose is to accept a test Cell Broadcast:

```java
private static final boolean TEST_MODE = SystemProperties.getInt("ro.debuggable", 0) == 1;
private static final String TEST_ACTION = "com.android.internal.telephony.gsm"
        + ".TEST_TRIGGER_CELL_BROADCAST";

private GsmInboundSmsHandler(...) {
    ...
    if (TEST_MODE) {
        if (mTestBroadcastReceiver == null) {
            mTestBroadcastReceiver = new GsmCbTestBroadcastReceiver();
            IntentFilter filter = new IntentFilter();
            filter.addAction(TEST_ACTION);
            context.registerReceiver(mTestBroadcastReceiver, filter,
                    Context.RECEIVER_EXPORTED);
        }
    }
}
```

Three consequences follow directly from this code, and each one matters:

1. **The receiver is registered at runtime, not declared in a manifest.** It therefore has no
   `package/class` component name. `am broadcast -n <package>` cannot reach it, and a command that
   names a package is rejected before any broadcast is attempted. This is the root cause recorded as
   BUG-021.
2. **`Context.RECEIVER_EXPORTED`** means any UID may target it, including the ADB shell
   (`uid=2000`). No permission is required. This is why the path is genuinely root-free when it
   exists at all.
3. **`TEST_MODE` is a build property, not a permission.** It is read once when the class initialises.
   On `ro.debuggable=0` the receiver object is never constructed, so a matching broadcast matches
   nothing: `am` reports `Broadcast completed: result=0` and nothing happens. An accepted command is
   not a delivered alert.

## The documented invocation

AOSP documents its own invocation in the Javadoc above the receiver:

```
adb shell am broadcast -a com.android.internal.telephony.gsm.TEST_TRIGGER_CELL_BROADCAST \
  --es pdu_string <hex pdu> [--ei phone_id 0]
```

and the handler reads the payload as:

```java
byte[] smsPdu = intent.getByteArrayExtra("pdu");
if (smsPdu == null) {
    String pduString = intent.getStringExtra("pdu_string");
    smsPdu = decodeHexString(pduString);
}
if (smsPdu == null) {
    log("No pdu or pdu_string extra, ignoring CB test intent");
    return;
}
```

So the accepted extras are `pdu` (a byte array), or `pdu_string` (hex text), plus the optional
`phone_id` integer. The project uses `pdu_string` and `phone_id`.

## Verification performed

Fetched with `curl` against `android.googlesource.com` and decoded from base64, then read directly.
The relevant lines were checked on three branches to confirm the gate has not changed:

| Branch | `TEST_MODE` definition | registration | delivery |
| --- | --- | --- | --- |
| `main` | `getInt("ro.debuggable", 0) == 1` | `RECEIVER_EXPORTED` | confirmed |
| `android14-release` | `getInt("ro.debuggable", 0) == 1` | `RECEIVER_EXPORTED` | confirmed |
| `android13-release` | `getInt("ro.debuggable", 0) == 1` | `RECEIVER_EXPORTED` | confirmed |

## What this does not establish

* It does **not** establish that the mechanism works on the operator's Galaxy A35. That requires the
  phone's `ro.debuggable` value, which this environment cannot read. A retail Samsung build ships
  `ro.debuggable=0` — Samsung's own published build types are `user` — but the value must be read
  from the device before any conclusion is drawn about it.
* It does **not** establish the Samsung package name, receiver manifest, or permission protection
  levels. Those are Samsung's, not AOSP's, and must be read from the device.
* It does **not** claim the receiver is reached on a `userdebug` OEM build. Samsung can and does
  modify this path; the AOSP file above is evidence about AOSP, not about Samsung.

## Where this leaves the investigation

The entry point is fully understood and its gate is a single readable property. The open question is
purely about the device: what is `ro.debuggable` on the A35, and what Cell Broadcast components does
its firmware declare? Both are answered by the read-only diagnostic batch, not by more source study.

Source: AOSP `frameworks/opt/telephony`, `GsmInboundSmsHandler.java`, fetched this session from
`android.googlesource.com`.
