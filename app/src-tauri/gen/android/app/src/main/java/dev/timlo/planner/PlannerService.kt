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
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat

/**
 * Runs only while the notification is up: keeps it pinned and the countdown
 * moving. [Planner] starts and stops it; exact alarms cover the gaps.
 */
class PlannerService : Service() {
  private val handler = Handler(Looper.getMainLooper())
  private val minute = object : Runnable {
    override fun run() {
      Planner.refresh(this@PlannerService)
      if (running) handler.postDelayed(this, 60_000 - System.currentTimeMillis() % 60_000 + 50)
    }
  }

  override fun onCreate() {
    super.onCreate()
    running = true
    Core.ensureInit(this)
    // startForeground must be called even if there turns out to be nothing to show.
    val n = Planner.current
      ?: NotificationCompat.Builder(this, "now").setSmallIcon(R.drawable.ic_stat_timlo).setContentTitle("timlo").build()
    val type = if (Build.VERSION.SDK_INT >= 34) ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE else 0
    ServiceCompat.startForeground(this, Planner.NOW_ID, n, type)
    if (Planner.current == null) {
      stopSelf()
      return
    }
    Core.startSync()
    handler.postDelayed(minute, 60_000 - System.currentTimeMillis() % 60_000 + 50)
  }

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int = START_NOT_STICKY

  override fun onDestroy() {
    running = false
    handler.removeCallbacks(minute)
    super.onDestroy()
  }

  override fun onBind(intent: Intent?): IBinder? = null

  companion object {
    @Volatile var running = false
      private set

    fun start(ctx: Context) {
      if (running) return
      try {
        ContextCompat.startForegroundService(ctx, Intent(ctx, PlannerService::class.java))
      } catch (e: Exception) {
        // Not allowed from the background right now; the notification is still posted.
        Log.w(Planner.TAG, "service not started", e)
      }
    }

    fun stop(ctx: Context) {
      if (running) ctx.stopService(Intent(ctx, PlannerService::class.java))
    }
  }
}
