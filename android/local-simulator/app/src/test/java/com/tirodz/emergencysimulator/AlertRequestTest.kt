package com.tirodz.emergencysimulator

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Tests for the alert-request validation.
 *
 * The log-injection tests are the important ones. The desktop controller decides whether an alert
 * appeared by reading `EMSIM:STAGE=` lines out of logcat, so a caller able to get a newline into a
 * message could emit a forged `NOTIFICATION_POSTED` line and manufacture the evidence that says the
 * alert succeeded. That would defeat the entire point of the project, which is that success is
 * never inferred from anything the sender controls.
 */
class AlertRequestTest {

    @Test
    fun `unset fields fall back to documented defaults`() {
        val validated = AlertRequest.validate(null, null, null, null)

        assertEquals(AlertRequest.DEFAULT_TITLE, validated.title)
        assertEquals(AlertRequest.DEFAULT_MESSAGE, validated.message)
        assertEquals(AlertRequest.DEFAULT_SEVERITY, validated.severity)
        assertEquals(AlertRequest.DEFAULT_CATEGORY, validated.category)
        assertTrue("no problem should be reported for absent extras", validated.problems.isEmpty())
    }

    @Test
    fun `blank fields fall back to defaults`() {
        val validated = AlertRequest.validate("   ", "", "  ", "  ")

        assertEquals(AlertRequest.DEFAULT_TITLE, validated.title)
        assertEquals(AlertRequest.DEFAULT_MESSAGE, validated.message)
        assertEquals(AlertRequest.DEFAULT_SEVERITY, validated.severity)
        assertEquals(AlertRequest.DEFAULT_CATEGORY, validated.category)
    }

    @Test
    fun `a newline in the message cannot forge a pipeline stage`() {
        val hostile = "Take shelter\nEMSIM:STAGE=NOTIFICATION_POSTED"

        val validated = AlertRequest.validate("T", hostile, "TEST", "ETWS-TEST")

        assertFalse(
            "the forged stage must not survive validation",
            validated.message.contains('\n'),
        )
        assertEquals("Take shelter EMSIM:STAGE=NOTIFICATION_POSTED", validated.message)
    }

    @Test
    fun `a carriage return is stripped and does not split the line`() {
        val validated = AlertRequest.validate(null, "line one\r\nline two", null, null)

        assertFalse(validated.message.contains('\r'))
        assertFalse(validated.message.contains('\n'))
        assertEquals("line one line two", validated.message)
    }

    @Test
    fun `control characters are dropped`() {
        val validated = AlertRequest.validate(null, "alert\u0000\u0007body", null, null)

        assertEquals("alertbody", validated.message)
    }

    @Test
    fun `runs of whitespace collapse to a single space`() {
        val validated = AlertRequest.validate(null, "Take     shelter\tnow", null, null)

        assertEquals("Take shelter now", validated.message)
    }

    @Test
    fun `a message longer than the limit is truncated and reported`() {
        val long = "A".repeat(AlertRequest.MAX_MESSAGE_CHARS + 250)

        val validated = AlertRequest.validate(null, long, null, null)

        assertEquals(AlertRequest.MAX_MESSAGE_CHARS, validated.message.length)
        assertTrue(
            "truncation must be reported, not silent",
            validated.problems.any { it.contains("truncated") },
        )
    }

    @Test
    fun `a title longer than the limit is bounded`() {
        val long = "T".repeat(AlertRequest.MAX_TITLE_CHARS + 100)

        val validated = AlertRequest.validate(long, null, null, null)

        assertTrue(validated.title.length <= AlertRequest.MAX_TITLE_CHARS)
    }

    @Test
    fun `a recognised severity is preserved and upper-cased`() {
        val validated = AlertRequest.validate(null, null, "severe", null)

        assertEquals("SEVERE", validated.severity)
        assertTrue(validated.problems.isEmpty())
    }

    @Test
    fun `an unrecognised severity falls back and is reported`() {
        val validated = AlertRequest.validate(null, null, "PANIC", null)

        assertEquals(AlertRequest.DEFAULT_SEVERITY, validated.severity)
        assertTrue(
            "an unrecognised severity must be reported",
            validated.problems.any { it.contains("severity") },
        )
    }

    @Test
    fun `a message of only control characters is reported rather than silently defaulted`() {
        val validated = AlertRequest.validate(null, "\n\r\u0000", null, null)

        assertEquals(AlertRequest.DEFAULT_MESSAGE, validated.message)
        assertTrue(
            "a message that reduced to nothing must be reported",
            validated.problems.any { it.contains("message") },
        )
    }

    @Test
    fun `sanitize bounds output even when the limit is smaller than the input`() {
        val result = AlertRequest.sanitize("abcdefghij", 4)

        assertEquals("abcd", result)
    }

    @Test
    fun `sanitize returns empty for null`() {
        assertEquals("", AlertRequest.sanitize(null, 10))
    }
}
