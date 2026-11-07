package dev.timlo.planner

import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.Context
import android.widget.RemoteViews

/** Home-screen widget: "+ Task" (quick add) and "Focus" (start a session for the current block). */
class QuickWidget : AppWidgetProvider() {
  override fun onUpdate(ctx: Context, manager: AppWidgetManager, ids: IntArray) {
    val views = RemoteViews(ctx.packageName, R.layout.widget_quick).apply {
      setOnClickPendingIntent(R.id.widget_add_task, Planner.openAction(ctx, Planner.ACTION_ADD_TASK))
      setOnClickPendingIntent(R.id.widget_focus, Planner.openAction(ctx, Planner.ACTION_FOCUS))
    }
    for (id in ids) manager.updateAppWidget(id, views)
  }
}
