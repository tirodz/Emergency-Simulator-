package com.tirodz.emergencysimulator

import android.util.Log

/**
 * One stable, machine-readable line per pipeline stage.
 *
 * The desktop controller decides whether an alert actually appeared from these lines, never from
 * an exit code. They are therefore a contract: the Rust side matches these exact tokens, so a
 * stage name here and a stage name there must change together. Prose logging would drift and the
 * controller would silently stop finding its own evidence.
 *
 * Format: `EMSIM:STAGE=<NAME>` optionally followed by ` key=value` pairs. ASCII only, no spaces
 * inside a name.
 */
object AlertStages {
    const val TAG = "EmergencySimulator"
    const val PREFIX = "EMSIM:STAGE="

    // Receiver
    const val RECEIVER_ACCEPTED = "ANDROID_RECEIVER_ACCEPTED"
    const val RECEIVER_REJECTED = "ANDROID_RECEIVER_REJECTED"

    // Notification
    const val NOTIFICATION_POSTED = "NOTIFICATION_POSTED"
    const val NOTIFICATION_FAILED = "NOTIFICATION_FAILED"
    const val NOTIFICATION_CANCELLED = "NOTIFICATION_CANCELLED"

    // Full-screen activity
    const val FULLSCREEN_ACTIVITY_STARTED = "FULLSCREEN_ACTIVITY_STARTED"
    const val FULLSCREEN_ACTIVITY_UNAVAILABLE = "FULLSCREEN_ACTIVITY_UNAVAILABLE"
    const val FULLSCREEN_ACTIVITY_STOPPED = "FULLSCREEN_ACTIVITY_STOPPED"

    // Audio
    const val AUDIO_START = "AUDIO_START"
    const val AUDIO_STOP = "AUDIO_STOP"
    const val AUDIO_UNAVAILABLE = "AUDIO_UNAVAILABLE"
    const val AUDIO_FOCUS_REQUEST = "AUDIO_FOCUS_REQUEST"
    const val AUDIO_FOCUS_RELEASE = "AUDIO_FOCUS_RELEASE"

    // Vibration
    const val VIBRATION_START = "VIBRATION_START"
    const val VIBRATION_STOP = "VIBRATION_STOP"

    // User action
    const val USER_DISMISSED = "USER_DISMISSED"

    fun log(stage: String, detail: String? = null) {
        val line = if (detail.isNullOrBlank()) {
            PREFIX + stage
        } else {
            PREFIX + stage + " " + detail
        }
        Log.i(TAG, line)
    }
}
