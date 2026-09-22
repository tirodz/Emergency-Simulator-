package com.tirodz.emergencysimulator

/**
 * Validation for an incoming alert request.
 *
 * The receiver is exported so the desktop controller can target it by explicit component. An
 * exported component is reachable by any app on the device, so the extras are treated as untrusted
 * input rather than as trusted controller input: they are length-bounded, stripped of characters
 * that would let a caller forge the pipeline's own log lines, and normalised to single lines.
 *
 * Kept separate from the receiver so it can be unit-tested without an Android runtime.
 */
object AlertRequest {
    const val DEFAULT_TITLE = "EMERGENCY ALERT TEST"
    const val DEFAULT_MESSAGE = "TEST ALERT - SIMULATION"
    const val DEFAULT_SEVERITY = "TEST"
    const val DEFAULT_CATEGORY = "ETWS-TEST"

    /** Comfortably larger than any legitimate alert, small enough that nothing can be smuggled. */
    const val MAX_TITLE_CHARS = 120
    const val MAX_MESSAGE_CHARS = 900

    /** Severities the controller is allowed to request. Anything else falls back to `TEST`. */
    private val ALLOWED_SEVERITIES = setOf("TEST", "INFO", "MINOR", "MODERATE", "SEVERE", "EXTREME")

    data class Validated(
        val title: String,
        val message: String,
        val severity: String,
        val category: String,
        val problems: List<String>,
    )

    fun validate(
        title: String?,
        message: String?,
        severity: String?,
        category: String?,
    ): Validated {
        val problems = mutableListOf<String>()

        val cleanTitle = sanitize(title, MAX_TITLE_CHARS)
        if (title != null && title.isNotBlank() && cleanTitle.isBlank()) {
            problems.add("title contained no usable characters")
        }

        val cleanMessage = sanitize(message, MAX_MESSAGE_CHARS)
        if (message != null && message.isNotBlank() && cleanMessage.isBlank()) {
            problems.add("message contained no usable characters")
        }
        if (message != null && message.length > MAX_MESSAGE_CHARS) {
            problems.add("message truncated from ${message.length} to $MAX_MESSAGE_CHARS chars")
        }

        // An unset severity is normal and defaults silently. A *set* severity that is not in the
        // allowed set is reported, because it means the caller and this app disagree about the
        // contract and the operator should not be left guessing why the alert reads "TEST".
        val requestedSeverity = severity?.trim()?.uppercase()
        val cleanSeverity = when {
            requestedSeverity.isNullOrBlank() -> DEFAULT_SEVERITY
            requestedSeverity in ALLOWED_SEVERITIES -> requestedSeverity
            else -> {
                problems.add(
                    "unrecognised severity '${severity?.take(32)}' replaced with $DEFAULT_SEVERITY"
                )
                DEFAULT_SEVERITY
            }
        }

        val cleanCategory = sanitize(category, 40).ifBlank { DEFAULT_CATEGORY }

        return Validated(
            title = cleanTitle.ifBlank { DEFAULT_TITLE },
            message = cleanMessage.ifBlank { DEFAULT_MESSAGE },
            severity = cleanSeverity,
            category = cleanCategory,
            problems = problems,
        )
    }

    /**
     * Collapses whitespace, drops control characters, and bounds the length.
     *
     * Newlines and carriage returns are removed rather than collapsed because a body containing
     * them could otherwise inject a forged `EMSIM:STAGE=` line into logcat, which is the exact
     * evidence channel the desktop controller trusts to decide what happened. A caller must not be
     * able to manufacture that evidence.
     */
    fun sanitize(value: String?, maxChars: Int): String {
        if (value == null) return ""
        val builder = StringBuilder(minOf(value.length, maxChars))
        var lastWasSpace = false
        for (character in value) {
            if (builder.length >= maxChars) break
            when {
                // Whitespace is tested before the control-character case because tab, newline and
                // carriage return satisfy both, and they must collapse to a space rather than be
                // dropped -- inserting a space is what stops "line one\nline two" becoming
                // "line oneline two".
                character.isWhitespace() -> lastWasSpace = true
                character.isISOControl() -> {}
                else -> {
                    if (lastWasSpace && builder.isNotEmpty()) builder.append(' ')
                    lastWasSpace = false
                    builder.append(character)
                }
            }
        }
        return builder.toString().trim()
    }
}
