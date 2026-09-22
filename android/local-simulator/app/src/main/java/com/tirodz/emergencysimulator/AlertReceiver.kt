package com.tirodz.emergencysimulator

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

class AlertReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != AlertNotificationHelper.ACTION) {
            AlertStages.log(AlertStages.RECEIVER_REJECTED, "action=${intent.action}")
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

        AlertStages.log(
            AlertStages.RECEIVER_ACCEPTED,
            "category=$category severity=$severity chars=${message.length}"
        )

        val outcome = runCatching {
            AlertNotificationHelper.show(context, title, message, severity, category)
        }

        if (outcome.isFailure) {
            // A receiver that throws produces no notification and no evidence, which reads
            // downstream as "Android blocked it" rather than "our own code failed". Name the real
            // cause instead of leaving an unexplained absence.
            val error = outcome.exceptionOrNull()
            AlertStages.log(
                AlertStages.NOTIFICATION_FAILED,
                "${error?.javaClass?.simpleName}: ${error?.message}"
            )
            return
        }

        AlertStages.log(
            AlertStages.NOTIFICATION_POSTED,
            "id=${AlertNotificationHelper.NOTIFICATION_ID}"
        )
    }
}
