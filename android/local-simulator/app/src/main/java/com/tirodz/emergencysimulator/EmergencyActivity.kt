package com.tirodz.emergencysimulator

import android.app.Activity
import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.media.MediaPlayer
import android.media.RingtoneManager
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
    private var vibrator: Vibrator? = null
    private var audioManager: AudioManager? = null
    private var focusRequest: AudioFocusRequest? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        configureWindow()
        super.onCreate(savedInstanceState)
        renderIntent()
    }

    override fun onNewIntent(intent: android.content.Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        stopEffects()
        renderIntent()
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
                Log.i(TAG, "EmergencyActivity dismissed")
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
        Log.i(TAG, "EmergencyActivity.onCreate/onNewIntent full-screen UI")
        startEffects()
    }

    private fun startEffects() {
        audioManager = getSystemService(Context.AUDIO_SERVICE) as AudioManager

        val attributes = AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_ALARM)
            .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
            .build()

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            focusRequest = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT_EXCLUSIVE)
                .setAudioAttributes(attributes)
                .setAcceptsDelayedFocusGain(false)
                .setWillPauseWhenDucked(false)
                .build()
            audioManager?.requestAudioFocus(focusRequest!!)
        } else {
            @Suppress("DEPRECATION")
            audioManager?.requestAudioFocus(
                null,
                AudioManager.STREAM_ALARM,
                AudioManager.AUDIOFOCUS_GAIN_TRANSIENT_EXCLUSIVE
            )
        }

        val uri = RingtoneManager.getDefaultUri(RingtoneManager.TYPE_ALARM)
            ?: RingtoneManager.getDefaultUri(RingtoneManager.TYPE_NOTIFICATION)

        mediaPlayer = MediaPlayer().apply {
            setAudioAttributes(attributes)
            setDataSource(applicationContext, uri)
            isLooping = true
            prepare()
            start()
        }

        vibrator = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            (getSystemService(Context.VIBRATOR_MANAGER_SERVICE) as VibratorManager).defaultVibrator
        } else {
            @Suppress("DEPRECATION")
            getSystemService(Context.VIBRATOR_SERVICE) as Vibrator
        }

        val pattern = longArrayOf(0, 700, 300, 700, 300, 1100)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            vibrator?.vibrate(VibrationEffect.createWaveform(pattern, -1))
        } else {
            @Suppress("DEPRECATION")
            vibrator?.vibrate(pattern, -1)
        }

        Log.i(TAG, "Alert audio/vibration started")
    }

    private fun stopEffects() {
        mediaPlayer?.runCatching {
            stop()
            release()
        }
        mediaPlayer = null
        vibrator?.cancel()

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            focusRequest?.let { audioManager?.abandonAudioFocusRequest(it) }
        } else {
            @Suppress("DEPRECATION")
            audioManager?.abandonAudioFocus(null)
        }
        focusRequest = null
    }

    override fun onDestroy() {
        stopEffects()
        super.onDestroy()
    }

    companion object {
        private const val TAG = "EmergencySimulator"
    }
}
