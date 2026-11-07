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

/**
 * One notification, only when there is something to show:
 *
 * - a block is running → "Study · 25m left", progress, Done / Skip;
 * - a reminder fired → it takes over the same notification ("Gym in 5 min",
 *   "Did you finish Study?") and alerts, then gives way to the block again;
 * - free time with nothing pending → no notification, service stopped. An
 *   exact alarm brings it back at the next block or reminder.
 */
object Planner {
  const val TAG = "timlo"
  private const val CH_NOW = "now"
  private const val CH_ALERTS = "alerts"
  private const val CH_ALERTS_SILENT = "alerts_silent"
  const val NOW_ID = 1
  /** Separate reminder notifications from older versions; removed on sight. */
  private const val OLD_ALERT_ID = 2

  const val ACTION_ANSWER = "dev.timlo.planner.ANSWER"
  const val ACTION_REPOST = "dev.timlo.planner.REPOST"
  const val ACTION_WAKE = "dev.timlo.planner.WAKE"
  const val EXTRA_BLOCK = "block_id"
  const val EXTRA_STATUS = "status"

  /** How long a reminder stays in the notification at most. */
  private const val NOTICE_MS = 10 * 60_000L
  private const val ASK_MS = 15 * 60_000L

  private val main = Handler(Looper.getMainLooper())
  private var pending: Runnable? = null

  /** What the service shows when it starts; null = nothing to show. */
  @Volatile var current: Notification? = null
    private set

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

  /** A reminder currently shown in place of the block (kept across process restarts). */
  private data class Headline(val title: String, val body: String, val blockId: String, val ask: Boolean, val untilMs: Long)

  private fun prefs(ctx: Context) = ctx.getSharedPreferences("planner", Context.MODE_PRIVATE)

  private fun loadHeadline(ctx: Context): Headline? {
    val p = prefs(ctx)
    val title = p.getString("h_title", null) ?: return null
    return Headline(title, p.getString("h_body", "")!!, p.getString("h_block", "")!!, p.getBoolean("h_ask", false), p.getLong("h_until", 0))
  }

  private fun saveHeadline(ctx: Context, h: Headline?) {
    val e = prefs(ctx).edit()
    if (h == null) {
      e.remove("h_title").remove("h_body").remove("h_block").remove("h_ask").remove("h_until")
    } else {
      e.putString("h_title", h.title).putString("h_body", h.body).putString("h_block", h.blockId)
        .putBoolean("h_ask", h.ask).putLong("h_until", h.untilMs)
    }
    e.apply()
  }

  /** Done / Skip was recorded for `blockId`: drop its question. */
  fun answered(ctx: Context, blockId: String) {
    val h = loadHeadline(ctx)
    if (h != null && (blockId.isEmpty() || h.blockId == blockId)) saveHeadline(ctx, null)
  }

  /** Recompute, post or remove the notification, start/stop the service and set the next alarm. */
  @Synchronized
  fun refresh(ctx: Context): Notification? {
    Core.ensureInit(ctx)
    ensureChannels(ctx)
    val nm = ctx.getSystemService(NotificationManager::class.java)
    for (sbn in nm.activeNotifications) if (sbn.id == OLD_ALERT_ID) nm.cancel(sbn.tag, OLD_ALERT_ID)

    val json = try {
      JSONObject(Core.tick())
    } catch (e: Exception) {
      JSONObject().put("error", e.toString())
    }
    if (json.has("error")) {
      Log.e(TAG, "tick: ${json.optString("error")}")
      schedule(ctx, System.currentTimeMillis() + 60_000)
      return current
    }
    val now = System.currentTimeMillis()
    val card = json.getJSONObject("card")
    val active = card.optString("state") == "active"

    // Reminders due this minute: the last one becomes the headline.
    var alert: JSONObject? = null
    val alerts = json.optJSONArray("alerts")
    if (alerts != null) {
      for (i in 0 until alerts.length()) {
        val a = alerts.getJSONObject(i)
        alert = a
        if (a.optString("kind") != "start") {
          val ask = a.optBoolean("ask")
          val life = if (ask) ASK_MS else minOf(if (a.isNull("timeout_ms")) NOTICE_MS else a.getLong("timeout_ms"), NOTICE_MS)
          saveHeadline(ctx, Headline(a.getString("title"), a.optString("body"), a.getString("block_id"), ask, now + life))
        }
      }
    }
    var headline = loadHeadline(ctx)
    if (headline != null && (now >= headline.untilMs || (headline.ask && !Core.blockOpen(headline.blockId)))) {
      saveHeadline(ctx, null)
      headline = null
    }

    val n = when {
      headline != null -> build(ctx, headline, card, active, alert)
      active -> build(ctx, null, card, true, alert)
      else -> null
    }
    current = n
    if (n == null) {
      nm.cancel(NOW_ID)
      PlannerService.stop(ctx)
    } else {
      nm.notify(NOW_ID, n)
      PlannerService.start(ctx)
    }
    // Wake for the next change, or when the headline should go.
    var wake = json.getLong("next_wake_ms")
    if (headline != null) wake = minOf(wake, headline.untilMs)
    schedule(ctx, wake)
    return n
  }

  private fun build(ctx: Context, h: Headline?, card: JSONObject, active: Boolean, alert: JSONObject?): Notification {
    val silent = alert == null || alert.optBoolean("silent")
    val channel = when {
      alert == null -> CH_NOW
      silent -> CH_ALERTS_SILENT
      else -> CH_ALERTS
    }
    val b = NotificationCompat.Builder(ctx, channel)
      .setSmallIcon(R.drawable.ic_stat_timlo)
      .setContentIntent(openApp(ctx))
      .setOngoing(true)
      .setShowWhen(false)
      .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
      .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
      // Android 14+ lets ongoing notifications be swiped; put it straight back.
      .setDeleteIntent(broadcast(ctx, ACTION_REPOST, 1))
    if (alert == null) {
      // Plain minute update: no sound, no pop-up.
      b.setOnlyAlertOnce(true).setSilent(true).setPriority(NotificationCompat.PRIORITY_LOW)
        .setCategory(NotificationCompat.CATEGORY_PROGRESS)
    } else {
      b.setOnlyAlertOnce(false).setPriority(NotificationCompat.PRIORITY_HIGH)
        .setCategory(NotificationCompat.CATEGORY_REMINDER)
      if (!silent) b.setDefaults(NotificationCompat.DEFAULT_ALL)
    }

    if (h != null) {
      b.setContentTitle(h.title)
      // While a block runs, keep it visible under the reminder.
      b.setContentText(if (active) card.optString("title") else h.body.ifEmpty { null })
      if (active && !card.isNull("progress")) b.setProgress(100, card.getInt("progress"), false)
      if (h.ask) {
        b.addAction(0, "Done", answerIntent(ctx, h.blockId, "done"))
        b.addAction(0, "Skip", answerIntent(ctx, h.blockId, "skipped"))
      } else if (active && !card.isNull("target")) {
        b.addAction(0, "Done", answerIntent(ctx, "", "done"))
        b.addAction(0, "Skip", answerIntent(ctx, "", "skipped"))
      }
      if (active && !h.ask) b.addAction(0, "Focus", openAction(ctx, ACTION_FOCUS))
    } else {
      b.setContentTitle(card.optString("title"))
        .setContentText(card.optString("text").ifEmpty { null })
        .setSubText(card.optString("sub").ifEmpty { null })
      if (!card.isNull("progress")) b.setProgress(100, card.getInt("progress"), false)
      if (!card.isNull("target")) {
        // Empty id = "the card's block" at tap time, so a stale button can't hit the wrong block.
        b.addAction(0, "Done", answerIntent(ctx, "", "done"))
        b.addAction(0, "Skip", answerIntent(ctx, "", "skipped"))
      }
      if (active) b.addAction(0, "Focus", openAction(ctx, ACTION_FOCUS))
    }
    return b.build()
  }

  private fun ensureChannels(ctx: Context) {
    if (Build.VERSION.SDK_INT < 26) return
    val nm = ctx.getSystemService(NotificationManager::class.java)
    if (nm.getNotificationChannel(CH_NOW) != null) return
    nm.createNotificationChannel(
      NotificationChannel(CH_NOW, "Now", NotificationManager.IMPORTANCE_LOW).apply {
        description = "The current block, updated quietly"
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

  const val EXTRA_ACTION = "timlo.action"
  const val ACTION_FOCUS = "focus"
  const val ACTION_ADD_TASK = "add-task"

  /** Open the app on a screen: "focus" starts a session for the current block, "add-task" opens quick add. */
  fun openAction(ctx: Context, action: String): PendingIntent {
    val i = Intent(ctx, MainActivity::class.java)
      .setAction("dev.timlo.planner.$action")
      .putExtra(EXTRA_ACTION, action)
      .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
    return PendingIntent.getActivity(ctx, action.hashCode(), i, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
  }

  private fun openApp(ctx: Context): PendingIntent {
    val i = Intent(ctx, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
    return PendingIntent.getActivity(ctx, 0, i, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
  }

  private fun broadcast(ctx: Context, action: String, code: Int, fill: Intent.() -> Unit = {}): PendingIntent {
    val i = Intent(ctx, ActionReceiver::class.java).setAction(action).apply(fill)
    return PendingIntent.getBroadcast(ctx, code, i, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
  }

  private fun answerIntent(ctx: Context, blockId: String, status: String): PendingIntent =
    broadcast(ctx, ACTION_ANSWER, "$blockId|$status".hashCode()) {
      putExtra(EXTRA_BLOCK, blockId)
      putExtra(EXTRA_STATUS, status)
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
