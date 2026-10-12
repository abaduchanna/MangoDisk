package app.mangodisk.desktop

import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.view.View
import android.view.ViewGroup
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  private val handler = Handler(Looper.getMainLooper())

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    bindBridge()
  }

  /// Poll the view tree until Tauri/Wry finishes inflating the WebView,
  /// then expose the MangoDiskBridge JS interface to the frontend.
  private fun bindBridge(attempt: Int = 0) {
    val wv = findWebView(window?.decorView)
    if (wv != null) {
      wv.addJavascriptInterface(Bridge(), "MangoDiskBridge")
    } else if (attempt < 120) {
      handler.postDelayed({ bindBridge(attempt + 1) }, 300)
    }
  }

  private fun findWebView(v: View?): WebView? {
    if (v is WebView) return v
    if (v is ViewGroup) {
      for (i in 0 until v.childCount) {
        findWebView(v.getChildAt(i))?.let { return it }
      }
    }
    return null
  }

  inner class Bridge {
    @JavascriptInterface
    fun hasAllFilesAccess(): Boolean =
      Build.VERSION.SDK_INT < Build.VERSION_CODES.R || Environment.isExternalStorageManager()

    @JavascriptInterface
    fun requestAllFilesAccess() {
      runOnUiThread {
        try {
          val i = Intent(
            Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
            Uri.parse("package:$packageName")
          )
          i.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
          startActivity(i)
        } catch (e: Exception) {
          try {
            startActivity(
              Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            )
          } catch (ignored: Exception) {
          }
        }
      }
    }
  }
}
