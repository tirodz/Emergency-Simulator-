package com.tirodz.emergencysimulator

import android.app.Activity
import android.content.Context
import android.net.Uri
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.media.MediaPlayer
import android.os.Build
import android.os.Bundle
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.view.Gravity
import android.view.WindowManager
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import android.graphics.Color
import android.graphics.Typeface

class EmergencyActivity : Activity() {
    private var mediaPlayer: MediaPlayer? = null
    private var pendingPlayer: MediaPlayer? = null
    private var audioCandidates: List<android.net.Uri> = emptyList()
    private var audioFailures = mutableListOf<String>()
    private var audioAttributes: AudioAttributes? = null

    /// Bumped whenever effects are torn down. Asynchronous prepare callbacks compare against it so
    /// a player superseded by a dismissal or a repeated alert cannot start sound afterwards.
    private var audioGeneration = 0

    private var vibrator: Vibrator? = null
    private var audioManager: AudioManager? = null
    private var focusRequest: AudioFocusRequest? = null
    private var audioStarted = false
    private var vibrationStarted = false

    override fun onCreate(savedInstanceState: Bundle?) {
        configureWindow()
        super.onCreate(savedInstanceState)
        AlertStages.log(
            AlertStages.FULLSCREEN_ACTIVITY_STARTED,
            "fullScreenAllowed=" + AlertNotificationHelper.canUseFullScreenIntent(this)
        )
        if (!AlertNotificationHelper.canUseFullScreenIntent(this)) {
            // The notification is still posted and still tappable; Android simply will not let it
            // take over the screen. Saying so is more useful than the controller inferring it from
            // a missing activity.
            AlertStages.log(
                AlertStages.FULLSCREEN_ACTIVITY_UNAVAILABLE,
                "USE_FULL_SCREEN_INTENT not granted"
            )
        }
        renderIntent()
    }

    override fun onNewIntent(intent: android.content.Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        // A repeated alert must not layer a second audio stream or a second vibration on top of
        // the first, so the previous effects are torn down before the new ones start.
        stopEffects()
        renderIntent()
    }

    /**
     * Sound and vibration belong to the visible alert, not to the process. Without this the
     * looping `MediaPlayer` and the vibration keep running after the operator leaves the alert
     * (home button, screen off, another notification), which is the "infinite sound after
     * dismissal" failure mode. `onStart`/`onStop` bracket visibility, so the alert is loud exactly
     * while it is on screen and silent the moment it is not.
     */
    override fun onStart() {
        super.onStart()
        // onStart/onStop bracket visibility; effects live exactly as long as the alert is on screen.
        startEffects()
    }

    override fun onStop() {
        stopEffects()
        AlertStages.log(AlertStages.FULLSCREEN_ACTIVITY_STOPPED)
        super.onStop()
    }

    override fun onDestroy() {
        stopEffects()
        super.onDestroy()
    }

    private fun configureWindow() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O_MR1) {
            setShowWhenLocked(true)
            setTurnScreenOn(true)
        } else {
            @Suppress("DEPRECATION")
            window.addFlags(
                WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED or
                    WindowManager.LayoutParams.FLAG_TURN_SCREEN_ON or
                    WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON
            )
        }

        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    }

    private fun renderIntent() {
        val title = intent.getStringExtra("title") ?: "EMERGENCY ALERT TEST"
        val message = intent.getStringExtra("message") ?: "TEST ALERT - SIMULATION"
        val severity = intent.getStringExtra("severity") ?: "TEST"
        val category = intent.getStringExtra("category") ?: "ETWS-TEST"

        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER
            setPadding(36, 44, 36, 44)
            setBackgroundColor(Color.rgb(7, 10, 8))
        }

        val card = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(30, 30, 30, 24)
            setBackgroundColor(Color.rgb(18, 25, 21))
        }

        val severityView = TextView(this).apply {
            text = severity
            textSize = 13f
            setTextColor(Color.rgb(255, 155, 99))
            typeface = Typeface.DEFAULT_BOLD
        }

        val titleView = TextView(this).apply {
            text = title
            textSize = 28f
            setTextColor(Color.WHITE)
            typeface = Typeface.DEFAULT_BOLD
            setPadding(0, 12, 0, 10)
        }

        val messageView = TextView(this).apply {
            text = message
            textSize = 18f
            setTextColor(Color.rgb(220, 229, 223))
        }

        val categoryView = TextView(this).apply {
            text = "LOCAL TEST · $category"
            textSize = 11f
            setTextColor(Color.rgb(130, 145, 136))
            setPadding(0, 22, 0, 28)
        }

        val dismiss = Button(this).apply {
            text = "DISMISS"
            isAllCaps = false
            setOnClickListener {
                stopEffects()
                AlertNotificationHelper.cancel(this@EmergencyActivity)
                AlertStages.log(AlertStages.USER_DISMISSED, "button")
                finishAndRemoveTask()
            }
        }

        card.addView(severityView)
        card.addView(titleView)
        card.addView(messageView)
        card.addView(categoryView)
        card.addView(dismiss)
        root.addView(card, LinearLayout.LayoutParams(
            LinearLayout.LayoutParams.MATCH_PARENT,
            LinearLayout.LayoutParams.WRAP_CONTENT
        ))

        setContentView(root)
        AlertStages.log(AlertStages.FULLSCREEN_ACTIVITY_STARTED, "rendered")
    }

    private fun startEffects() {
        if (audioStarted || vibrationStarted) return

        audioManager = getSystemService(Context.AUDIO_SERVICE) as AudioManager

        val attributes = AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_ALARM)
            .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
            .build()

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val request = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT_EXCLUSIVE)
                .setAudioAttributes(attributes)
                .setAcceptsDelayedFocusGain(false)
                .setWillPauseWhenDucked(false)
                .build()
            focusRequest = request
            val result = audioManager?.requestAudioFocus(request)
            AlertStages.log(
                AlertStages.AUDIO_FOCUS_REQUEST,
                "granted=${result == AudioManager.AUDIOFOCUS_REQUEST_GRANTED}"
            )
        } else {
            @Suppress("DEPRECATION")
            val result = audioManager?.requestAudioFocus(
                null,
                AudioManager.STREAM_ALARM,
                AudioManager.AUDIOFOCUS_GAIN_TRANSIENT_EXCLUSIVE
            )
            AlertStages.log(
                AlertStages.AUDIO_FOCUS_REQUEST,
                "granted=${result == AudioManager.AUDIOFOCUS_REQUEST_GRANTED}"
            )
        }

        startAudio(attributes)
        startVibration()
    }

    /**
     * Audio is best-effort by design. A device with no alarm ringtone, or one where the ringtone
     * cannot be prepared, must still show the alert: a silent full-screen alert is a useful
     * result, an activity that throws during `onStart` is not. Every failure is reported as a
     * stage so the desktop can distinguish "no sound configured" from "the alert never appeared".
     *
     * Preparation is asynchronous on purpose. A blocking `prepare()` on a slow or unreadable media
     * source froze the alert UI for over ten seconds on an emulator image whose alarm URI does not
     * resolve -- long enough to risk an ANR on a real device. `prepareAsync()` keeps the alert
     * visible and responsive while the tone is being resolved.
     *
     * Candidates are tried in order because a URI that exists can still fail to open; the alert
     * falls back to the notification tone rather than falling silent.
     */
    private fun startAudio(attributes: AudioAttributes) {
        // The alert tone is bundled rather than taken from the device. The device's own alarm or
        // notification ringtone is a pleasant chime on most OEM builds, which makes a simulated
        // emergency alert indistinguishable by ear from an incoming text message. A bundled,
        // recognisable attention tone is what makes this a test instrument rather than a
        // notification demo. The tone is also the only source that is guaranteed to exist, so the
        // previous multi-candidate fallback to the notification sound is no longer needed.
        audioCandidates = listOfNotNull(
            Uri.parse("android.resource://" + packageName + "/" + R.raw.emergency_tone),
        )

        if (audioCandidates.isEmpty()) {
            AlertStages.log(AlertStages.AUDIO_UNAVAILABLE, "no bundled tone resource")
            return
        }

        audioAttributes = attributes
        audioFailures.clear()
        prepareAudioCandidate(0)
    }

    private fun prepareAudioCandidate(index: Int) {
        if (index >= audioCandidates.size) {
            AlertStages.log(AlertStages.AUDIO_UNAVAILABLE, audioFailures.joinToString("; "))
            return
        }

        // `generation` invalidates callbacks from a player that a later stop() or onNewIntent has
        // already superseded, so a stale prepare can never start sound after dismissal.
        val generation = audioGeneration
        val uri = audioCandidates[index]
        val player = MediaPlayer()
        pendingPlayer = player

        val advance = {
            runCatching { player.release() }
            if (generation == audioGeneration) {
                prepareAudioCandidate(index + 1)
            }
        }

        runCatching {
            player.setAudioAttributes(audioAttributes)
            player.setOnPreparedListener { prepared ->
                if (generation != audioGeneration) {
                    runCatching { prepared.release() }
                    return@setOnPreparedListener
                }
                val started = runCatching { prepared.start() }
                if (started.isFailure) {
                    audioFailures += "start failed for $uri"
                    advance()
                    return@setOnPreparedListener
                }
                pendingPlayer = null
                mediaPlayer = prepared
                audioStarted = true
                AlertStages.log(AlertStages.AUDIO_START, "usage=ALARM looping=true source=$uri")
            }
            player.setOnErrorListener { _, what, extra ->
                audioFailures += "error $what/$extra for $uri"
                advance()
                true
            }
            player.isLooping = true
            player.setDataSource(applicationContext, uri)
            player.prepareAsync()
        }.onFailure { error ->
            audioFailures += "${error.javaClass.simpleName} for $uri"
            advance()
        }
    }

    private fun startVibration() {
        vibrator = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            (getSystemService(Context.VIBRATOR_MANAGER_SERVICE) as? VibratorManager)?.defaultVibrator
        } else {
            @Suppress("DEPRECATION")
            getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator
        }

        val target = vibrator
        if (target == null || !target.hasVibrator()) {
            AlertStages.log(AlertStages.VIBRATION_STOP, "no vibrator on this device")
            return
        }

        // Repeat index -1 plays the pattern once. A repeating pattern would keep buzzing after the
        // alert is gone on any device where the cancel is missed.
        val pattern = AlertNotificationHelper.VIBRATION_PATTERN
        val outcome = runCatching {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                target.vibrate(VibrationEffect.createWaveform(pattern, -1))
            } else {
                @Suppress("DEPRECATION")
                target.vibrate(pattern, -1)
            }
        }

        if (outcome.isFailure) {
            AlertStages.log(
                AlertStages.VIBRATION_STOP,
                "vibrate failed: ${outcome.exceptionOrNull()?.message}"
            )
            return
        }

        vibrationStarted = true
        AlertStages.log(AlertStages.VIBRATION_START, "pattern=700,300,700,300,1100")
    }

    private fun stopEffects() {
        // Invalidating the generation first means any in-flight prepare callback becomes a no-op,
        // so a tone cannot begin playing after the alert was dismissed.
        audioGeneration += 1

        pendingPlayer?.let { runCatching { it.release() } }
        pendingPlayer = null

        if (audioStarted || mediaPlayer != null) {
            mediaPlayer?.let { player ->
                // `stop()` throws if the player was never successfully started, so each step is
                // guarded independently: a failure to stop must still not leak the handle.
                runCatching { if (player.isPlaying) player.stop() }
                runCatching { player.release() }
            }
            mediaPlayer = null
            audioStarted = false
            AlertStages.log(AlertStages.AUDIO_STOP)
        }

        if (vibrationStarted || vibrator != null) {
            vibrator?.let { runCatching { it.cancel() } }
            vibrator = null
            vibrationStarted = false
            AlertStages.log(AlertStages.VIBRATION_STOP)
        }

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            focusRequest?.let { request ->
                audioManager?.abandonAudioFocusRequest(request)
                AlertStages.log(AlertStages.AUDIO_FOCUS_RELEASE)
            }
        } else {
            @Suppress("DEPRECATION")
            audioManager?.abandonAudioFocus(null)
            AlertStages.log(AlertStages.AUDIO_FOCUS_RELEASE)
        }
        focusRequest = null
    }
}
