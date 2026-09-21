package com.tirodz.emergencysimulator

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.media.AudioAttributes
import android.media.RingtoneManager
import android.os.Build

object AlertNotificationHelper {
    const val CHANNEL_ID = "local_emergency_test"
    const val NOTIFICATION_ID = 4355
    const val ACTION = "com.tirodz.emergencysimulator.TRIGGER_ALERT"

    fun show(
        context: Context,
        title: String,
        message: String,
        severity: String,
        category: String
    ) {
        val manager = context.getSystemService(NotificationManager::class.java)
        ensureChannel(context, manager)

        val fullScreenIntent = Intent(context, EmergencyActivity::class.java).apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
            putExtra("title", title)
            putExtra("message", message)
            putExtra("severity", severity)
            putExtra("category", category)
        }

        val fullScreenPendingIntent = PendingIntent.getActivity(
            context,
            NOTIFICATION_ID,
            fullScreenIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        val notification = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(context, CHANNEL_ID)
                .setSmallIcon(R.drawable.ic_alert)
                .setContentTitle("[$severity] $title")
                .setContentText(message)
                .setCategory(Notification.CATEGORY_ALARM)
                .setVisibility(Notification.VISIBILITY_PUBLIC)
                .setContentIntent(fullScreenPendingIntent)
                .setOngoing(true)
                .setAutoCancel(false)
                .setFullScreenIntent(fullScreenPendingIntent, true)
                .build()
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(context)
                .setSmallIcon(R.drawable.ic_alert)
                .setContentTitle("[$severity] $title")
                .setContentText(message)
                .setPriority(Notification.PRIORITY_HIGH)
                .setCategory(Notification.CATEGORY_ALARM)
                .setFullScreenIntent(fullScreenPendingIntent, true)
                .build()
        }

        // Replacing the previous notification of the same id is what keeps a repeated send from
        // stacking alerts on the lock screen. Android treats notify() with an identical id as an
        // update, so no explicit cancel is needed and cancelling first would flash the shade.
        manager.notify(NOTIFICATION_ID, notification)
    }

    fun cancel(context: Context) {
        runCatching {
            context.getSystemService(NotificationManager::class.java).cancel(NOTIFICATION_ID)
        }
    }

    /**
     * Whether Android will actually let this notification become a full-screen alert.
     *
     * On Android 14+ `USE_FULL_SCREEN_INTENT` became a special app op that defaults to *denied*
     * for apps that are not calling or alarm apps, so a notification can post successfully and
     * still never take over the screen. The controller needs to be able to tell the operator which
     * of those two things happened, so this is reported as a stage rather than assumed.
     *
     * `canUseFullScreenIntent()` is the public API for exactly this question (added in API 34).
     * Reading the app op directly would mean using the hidden `OPSTR_USE_FULL_SCREEN_INTENT`
     * constant, which is not part of the SDK and is subject to the hidden-API restrictions.
     */
    fun canUseFullScreenIntent(context: Context): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.UPSIDE_DOWN_CAKE) return true

        val manager = context.getSystemService(NotificationManager::class.java) ?: return false
        return manager.canUseFullScreenIntent()
    }

    private fun ensureChannel(context: Context, manager: NotificationManager) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return

        // `getDefaultUri` returns null when the device has no alarm sound configured. Passing that
        // null through to `setSound` throws, and a throwing channel setup would abort the whole
        // notification. The alert must still appear silently rather than not appear at all.
        val sound = RingtoneManager.getDefaultUri(RingtoneManager.TYPE_ALARM)
            ?: RingtoneManager.getDefaultUri(RingtoneManager.TYPE_NOTIFICATION)

        val attributes = AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_ALARM)
            .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
            .build()

        val channel = NotificationChannel(
            CHANNEL_ID,
            "Local emergency test alerts",
            NotificationManager.IMPORTANCE_HIGH
        ).apply {
            description = "Local offline Emergency Simulator alerts"
            if (sound != null) {
                setSound(sound, attributes)
            }
            enableVibration(true)
            vibrationPattern = longArrayOf(0, 700, 300, 700, 300, 1100)
            lockscreenVisibility = Notification.VISIBILITY_PUBLIC
            setBypassDnd(true)
        }

        manager.createNotificationChannel(channel)
    }
}
