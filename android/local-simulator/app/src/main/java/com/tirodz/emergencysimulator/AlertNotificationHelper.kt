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

        manager.cancel(NOTIFICATION_ID)
        manager.notify(NOTIFICATION_ID, notification)
    }

    fun cancel(context: Context) {
        context.getSystemService(NotificationManager::class.java).cancel(NOTIFICATION_ID)
    }

    fun canUseFullScreenIntent(context: Context): Boolean {
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            context.getSystemService(NotificationManager::class.java).canUseFullScreenIntent()
        } else {
            true
        }
    }

    private fun ensureChannel(context: Context, manager: NotificationManager) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return

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
            setSound(sound, attributes)
            enableVibration(true)
            vibrationPattern = longArrayOf(0, 700, 300, 700, 300, 1100)
            lockscreenVisibility = Notification.VISIBILITY_PUBLIC
        }

        manager.createNotificationChannel(channel)
    }
}
