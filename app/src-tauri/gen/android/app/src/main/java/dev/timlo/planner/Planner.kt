package dev.timlo.planner

import android.app.AlarmManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Log
import androidx.core.app.NotificationCompat
import org.json.JSONObject

/** Builds the notifications and schedules the next wake-up from `Core.tick()`. */
object Planner {
  const val TAG = "timlo"
  private const val CH_NOW = "now"
  private const val CH_ALERTS = "alerts"
  private const val CH_ALERTS_SILENT = "alerts_silent"
  const val NOW_ID = 1
  private const val ALERT_ID = 2

  const val ACTION_ANSWER = "dev.timlo.planner.ANSWER"
  const val ACTION_REPOST = "dev.timlo.planner.REPOST"
  const val ACTION_WAKE = "dev.timlo.planner.WAKE"
  const val EXTRA_BLOCK = "block_id"
  const val EXTRA_STATUS = "status"
  const val EXTRA_TAG = "tag"

  private val main = Handler(Looper.getMainLooper())
  private var pending: Runnable? = null

  /** Refresh soon on the main thread; bursts of changes collapse into one. */
  fun requestRefresh(ctx: Context) {
    val app = ctx.applicationContext
    main.post {
      pending?.let { main.removeCallbacks(it) }
      val r = Runnable { refresh(app) }
      pending = r
      main.postDelayed(r, 150)
    }
  }

  /** Redraw the now notification, post due reminders and set the next alarm. */
  @Synchronized
  fun refresh(ctx: Context): Notification {
    Core.ensureInit(ctx)
    ensureChannels(ctx)
    val nm = ctx.getSystemService(NotificationManager::class.java)
    val json = try {
      JSONObject(Core.tick())
    } catch (e: Exception) {
      JSONObject().put("error", e.toString())
    }
    if (json.has("error")) {
      Log.e(TAG, "tick: ${json.optString("error")}")
      val n = base(ctx, CH_NOW).setContentTitle("timlo").setContentText(json.optString("error")).setOngoing(true).build()
      nm.notify(NOW_ID, n)
      schedule(ctx, System.currentTimeMillis() + 60_000)
      return n
    }

    val alerts = json.optJSONArray("alerts")
    if (alerts != null) {
      for (i in 0 until alerts.length()) postAlert(ctx, nm, alerts.getJSONObject(i))
    }
    withdrawAnswered(nm)

    val n = nowNotification(ctx, json.getJSONObject("card"))
    nm.notify(NOW_ID, n)
    schedule(ctx, json.getLong("next_wake_ms"))
    return n
  }

  private fun ensureChannels(ctx: Context) {
    if (Build.VERSION.SDK_INT < 26) return
    val nm = ctx.getSystemService(NotificationManager::class.java)
    if (nm.getNotificationChannel(CH_NOW) != null) return
    nm.createNotificationChannel(
      NotificationChannel(CH_NOW, "Now", NotificationManager.IMPORTANCE_LOW).apply {
        description = "What you should be doing right now"
        setShowBadge(false)
      })
    nm.createNotificationChannel(
      NotificationChannel(CH_ALERTS, "Reminders", NotificationManager.IMPORTANCE_HIGH).apply {
        description = "Block starts, \"in 5 min\" and \"Did you finish…?\""
      })
    nm.createNotificationChannel(
      NotificationChannel(CH_ALERTS_SILENT, "Reminders (no sound)", NotificationManager.IMPORTANCE_HIGH).apply {
        description = "Used when sound is turned off in timlo's settings"
        setSound(null, null)
        enableVibration(false)
      })
  }

  private fun base(ctx: Context, channel: String) =
    NotificationCompat.Builder(ctx, channel)
      .setSmallIcon(R.drawable.ic_stat_timlo)
      .setContentIntent(openApp(ctx))

  private fun openApp(ctx: Context): PendingIntent {
    val i = Intent(ctx, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
    return PendingIntent.getActivity(ctx, 0, i, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
  }

  private fun broadcast(ctx: Context, action: String, code: Int, fill: Intent.() -> Unit = {}): PendingIntent {
    val i = Intent(ctx, ActionReceiver::class.java).setAction(action).apply(fill)
    return PendingIntent.getBroadcast(ctx, code, i, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
  }

  private fun answerIntent(ctx: Context, blockId: String, status: String, tag: String?): PendingIntent =
    broadcast(ctx, ACTION_ANSWER, "$blockId|$status|$tag".hashCode()) {
      putExtra(EXTRA_BLOCK, blockId)
      putExtra(EXTRA_STATUS, status)
      if (tag != null) putExtra(EXTRA_TAG, tag)
    }

  private fun nowNotification(ctx: Context, card: JSONObject): Notification {
    val b = base(ctx, CH_NOW)
      .setContentTitle(card.optString("title"))
      .setContentText(card.optString("text"))
      .setSubText(card.optString("sub").ifEmpty { null })
      .setOngoing(true)
      .setOnlyAlertOnce(true)
      .setSilent(true)
      .setShowWhen(false)
      .setCategory(NotificationCompat.CATEGORY_PROGRESS)
      .setPriority(NotificationCompat.PRIORITY_LOW)
      .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
      .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
      // Android 14+ lets ongoing notifications be swiped; put it straight back.
      .setDeleteIntent(broadcast(ctx, ACTION_REPOST, 1))
    if (!card.isNull("progress")) b.setProgress(100, card.getInt("progress"), false)
    if (!card.isNull("target")) {
      // Empty id = "the card's block" at tap time, so a stale button can't hit the wrong block.
      b.addAction(0, "Done", answerIntent(ctx, "", "done", null))
      b.addAction(0, "Skip", answerIntent(ctx, "", "skipped", null))
    }
    return b.build()
  }

  private fun postAlert(ctx: Context, nm: NotificationManager, a: JSONObject) {
    val tag = a.getString("tag")
    val blockId = a.getString("block_id")
    val b = base(ctx, if (a.optBoolean("silent")) CH_ALERTS_SILENT else CH_ALERTS)
      .setContentTitle(a.getString("title"))
      .setContentText(a.optString("body").ifEmpty { null })
      .setAutoCancel(true)
      .setCategory(NotificationCompat.CATEGORY_REMINDER)
      .setPriority(NotificationCompat.PRIORITY_HIGH)
      .setDefaults(if (a.optBoolean("silent")) 0 else NotificationCompat.DEFAULT_ALL)
    if (!a.isNull("timeout_ms")) b.setTimeoutAfter(a.getLong("timeout_ms"))
    if (a.optBoolean("ask")) {
      b.addAction(0, "Done", answerIntent(ctx, blockId, "done", tag))
      b.addAction(0, "Skip", answerIntent(ctx, blockId, "skipped", tag))
    }
    nm.notify(tag, ALERT_ID, b.build())
  }

  /** "Did you finish…?" prompts answered here, in the app or on the PC. */
  private fun withdrawAnswered(nm: NotificationManager) {
    for (sbn in nm.activeNotifications) {
      val tag = sbn.tag ?: continue
      if (sbn.id == ALERT_ID && tag.startsWith("ask:") && !Core.blockOpen(tag.removePrefix("ask:"))) {
        nm.cancel(tag, ALERT_ID)
      }
    }
  }

  fun cancelAlert(ctx: Context, tag: String) {
    ctx.getSystemService(NotificationManager::class.java).cancel(tag, ALERT_ID)
  }

  /** One exact alarm at the next change; it fires with the app closed and in Doze. */
  private fun schedule(ctx: Context, atMs: Long) {
    val am = ctx.getSystemService(AlarmManager::class.java)
    val i = Intent(ctx, AlarmReceiver::class.java).setAction(ACTION_WAKE)
    val pi = PendingIntent.getBroadcast(ctx, 0, i, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    val at = maxOf(atMs, System.currentTimeMillis() + 1_000)
    try {
      if (Build.VERSION.SDK_INT >= 31 && !am.canScheduleExactAlarms()) {
        am.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, pi)
      } else if (Build.VERSION.SDK_INT >= 23) {
        am.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, pi)
      } else {
        am.setExact(AlarmManager.RTC_WAKEUP, at, pi)
      }
    } catch (e: SecurityException) {
      Log.w(TAG, "exact alarm refused, using inexact", e)
      am.set(AlarmManager.RTC_WAKEUP, at, pi)
    }
  }
}
