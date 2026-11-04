package dev.timlo.planner

import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.util.Log
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat

/**
 * Keeps the "now" notification pinned and the countdown moving. Exact alarms
 * (see [Planner.schedule]) cover block boundaries and reminders even when
 * Android pauses this service's minute ticks in Doze.
 */
class PlannerService : Service() {
  private val handler = Handler(Looper.getMainLooper())
  private val minute = object : Runnable {
    override fun run() {
      Planner.refresh(this@PlannerService)
      val now = System.currentTimeMillis()
      handler.postDelayed(this, 60_000 - now % 60_000 + 50)
    }
  }

  override fun onCreate() {
    super.onCreate()
    Core.ensureInit(this)
    val n = Planner.refresh(this)
    val type = if (Build.VERSION.SDK_INT >= 34) ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE else 0
    ServiceCompat.startForeground(this, Planner.NOW_ID, n, type)
    Core.startSync()
    handler.postDelayed(minute, 60_000 - System.currentTimeMillis() % 60_000 + 50)
  }

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    Planner.requestRefresh(this)
    return START_STICKY
  }

  override fun onDestroy() {
    handler.removeCallbacks(minute)
    super.onDestroy()
  }

  override fun onBind(intent: Intent?): IBinder? = null

  companion object {
    fun start(ctx: Context) {
      try {
        ContextCompat.startForegroundService(ctx, Intent(ctx, PlannerService::class.java))
      } catch (e: Exception) {
        // Not allowed from the background right now; the alarm still refreshes the notification.
        Log.w(Planner.TAG, "service not started", e)
      }
    }
  }
}
