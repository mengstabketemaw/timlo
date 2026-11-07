package dev.timlo.planner

import android.content.Context

/**
 * The shared Rust core (`app/src-tauri/src/android.rs`). Same library and
 * database as the Tauri UI, so the service works with the app closed.
 */
object Core {
  init {
    System.loadLibrary("timlo_app_lib")
  }

  @Volatile private var app: Context? = null

  /** Call before anything else, from every entry point (activity, service, receivers). */
  fun ensureInit(ctx: Context) {
    if (app != null) return
    synchronized(this) {
      if (app == null) {
        init(ctx.filesDir.absolutePath)
        app = ctx.applicationContext
      }
    }
  }

  @JvmStatic external fun init(dir: String)

  /** JSON: `{card, alerts, next_wake_ms}` or `{error}`. */
  @JvmStatic external fun tick(): String

  /** Done / Skip ("done" | "skipped"); empty id = the now card's block. Returns an error or "". */
  @JvmStatic external fun answer(blockId: String, status: String): String

  @JvmStatic external fun blockOpen(blockId: String): Boolean

  @JvmStatic external fun startSync()

  /** Tell the UI to open a screen ("focus", "add-task"); it picks this up when it gets focus. */
  @JvmStatic external fun setLaunchAction(action: String)

  /** Called by Rust (any thread) after a local change or a sync pull. */
  @JvmStatic
  fun onNativeChange() {
    app?.let { Planner.requestRefresh(it) }
  }
}
