package com.tirodz.emergencysimulator

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

class AlertReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != AlertNotificationHelper.ACTION) {
            Log.w(TAG, "Ignoring unexpected action: ${intent.action}")
            return
        }

        val title = intent.getStringExtra("title")?.takeIf { it.isNotBlank() }
            ?: "EMERGENCY ALERT TEST"
        val message = intent.getStringExtra("message")?.takeIf { it.isNotBlank() }
            ?: "TEST ALERT - SIMULATION"
        val severity = intent.getStringExtra("severity")?.takeIf { it.isNotBlank() }
            ?: "TEST"
        val category = intent.getStringExtra("category")?.takeIf { it.isNotBlank() }
            ?: "ETWS-TEST"

        Log.i(TAG, "AlertReceiver.onReceive title=$title category=$category fullScreen="+AlertNotificationHelper.canUseFullScreenIntent(context))
        AlertNotificationHelper.show(context, title, message, severity, category)
        Log.i(TAG, "AlertNotificationHelper.notify posted notification")
    }

    companion object {
        private const val TAG = "EmergencySimulator"
    }
}
