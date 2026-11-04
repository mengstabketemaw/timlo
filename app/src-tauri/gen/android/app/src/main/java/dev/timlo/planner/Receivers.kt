package dev.timlo.planner

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

/** The exact alarm: a block boundary or reminder is due. */
class AlarmReceiver : BroadcastReceiver() {
  override fun onReceive(ctx: Context, intent: Intent) {
    Planner.refresh(ctx)
    // Restart the service if the system killed it (alarms may start one).
    PlannerService.start(ctx)
  }
}

/** Done / Skip buttons, and re-posting a swiped-away now notification. */
class ActionReceiver : BroadcastReceiver() {
  override fun onReceive(ctx: Context, intent: Intent) {
    Core.ensureInit(ctx)
    when (intent.action) {
      Planner.ACTION_ANSWER -> {
        val err = Core.answer(
          intent.getStringExtra(Planner.EXTRA_BLOCK) ?: "",
          intent.getStringExtra(Planner.EXTRA_STATUS) ?: return,
        )
        if (err.isNotEmpty()) Log.e(Planner.TAG, "answer: $err")
        intent.getStringExtra(Planner.EXTRA_TAG)?.let { Planner.cancelAlert(ctx, it) }
      }
    }
    Planner.refresh(ctx)
  }
}

/** Start again after a reboot or an update; re-plan after a clock change. */
class BootReceiver : BroadcastReceiver() {
  override fun onReceive(ctx: Context, intent: Intent) {
    when (intent.action) {
      Intent.ACTION_BOOT_COMPLETED, Intent.ACTION_MY_PACKAGE_REPLACED -> PlannerService.start(ctx)
      else -> Planner.refresh(ctx)
    }
  }
}
