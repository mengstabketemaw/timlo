package dev.timlo.planner

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

/** The exact alarm: a block boundary or reminder is due. */
class AlarmReceiver : BroadcastReceiver() {
  override fun onReceive(ctx: Context, intent: Intent) {
    Planner.refresh(ctx)
    // The service (and with it sync) only runs while a block does; catch up here.
    Core.startSync()
  }
}

/** Done / Skip buttons, and re-posting a swiped-away notification. */
class ActionReceiver : BroadcastReceiver() {
  override fun onReceive(ctx: Context, intent: Intent) {
    Core.ensureInit(ctx)
    if (intent.action == Planner.ACTION_ANSWER) {
      val block = intent.getStringExtra(Planner.EXTRA_BLOCK) ?: ""
      val status = intent.getStringExtra(Planner.EXTRA_STATUS) ?: return
      val err = Core.answer(block, status)
      if (err.isNotEmpty()) Log.e(Planner.TAG, "answer: $err") else Planner.answered(ctx, block)
    }
    Planner.refresh(ctx)
  }
}

/** Re-plan after a reboot, an update or a clock change. */
class BootReceiver : BroadcastReceiver() {
  override fun onReceive(ctx: Context, intent: Intent) {
    Planner.refresh(ctx)
  }
}
