package com.tirodz.emergencysimulator

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/**
 * The local simulator's broadcast entry point.
 *
 * This is a **local app alert**, not a Cell Broadcast. It is triggered by the desktop controller
 * through an explicit component and produces a high-importance notification with a full-screen
 * intent, the bundled attention tone, and vibration. The genuine Android Cell Broadcast path is a
 * separate, protected pipeline that the ADB shell user cannot send to; see the project README.
 */
class AlertReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != AlertNotificationHelper.ACTION) {
            AlertStages.log(AlertStages.RECEIVER_REJECTED, "action=${intent.action}")
            return
        }

        val validated = AlertRequest.validate(
            title = intent.getStringExtra("title"),
            message = intent.getStringExtra("message"),
            severity = intent.getStringExtra("severity"),
            category = intent.getStringExtra("category"),
        )

        // Reported before the notification attempt so a rejected or normalised field is visible in
        // the diagnostic log rather than silently changing what the operator sees on the phone.
        for (problem in validated.problems) {
            AlertStages.log(AlertStages.RECEIVER_INPUT_REJECTED, problem)
        }

        AlertStages.log(
            AlertStages.RECEIVER_ACCEPTED,
            "category=${validated.category} severity=${validated.severity} " +
                "chars=${validated.message.length}",
        )

        val outcome = runCatching {
            AlertNotificationHelper.show(
                context,
                validated.title,
                validated.message,
                validated.severity,
                validated.category,
            )
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
