package dev.timlo.planner

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.enableEdgeToEdge
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    // Before the Tauri side opens the database.
    Core.ensureInit(this)
    takeAction(intent)
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    keepClearOfSystemBars()

    // Notifications, and reading the Focus music folder.
    val wanted = if (Build.VERSION.SDK_INT >= 33)
      arrayOf(Manifest.permission.POST_NOTIFICATIONS, Manifest.permission.READ_MEDIA_AUDIO)
    else
      arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE)
    val missing = wanted.filter {
      ContextCompat.checkSelfPermission(this, it) != PackageManager.PERMISSION_GRANTED
    }
    if (missing.isNotEmpty()) ActivityCompat.requestPermissions(this, missing.toTypedArray(), 1)
    Planner.requestRefresh(this)
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    takeAction(intent)
  }

  /** From the widget or the notification's Focus button. */
  private fun takeAction(intent: Intent?) {
    intent?.getStringExtra(Planner.EXTRA_ACTION)?.let { Core.setLaunchAction(it) }
  }

  /**
   * Edge-to-edge is forced on Android 15+, so the web view would sit under the
   * status bar, camera cutout and navigation bar. Pad it by their size (and
   * the keyboard's, so focused inputs stay visible).
   */
  private fun keepClearOfSystemBars() {
    val content = findViewById<android.view.View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(content) { v, insets ->
      val bars = insets.getInsets(
        WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout() or WindowInsetsCompat.Type.ime()
      )
      v.setPadding(bars.left, bars.top, bars.right, bars.bottom)
      WindowInsetsCompat.CONSUMED
    }
    ViewCompat.requestApplyInsets(content)
  }

  override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
    super.onRequestPermissionsResult(requestCode, permissions, grantResults)
    Planner.requestRefresh(this)
  }
}
