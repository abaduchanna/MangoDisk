package app.mangodisk.desktop

import android.Manifest
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.content.pm.PackageManager
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
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
    // Android 11+: "All files access". Android 7-10: classic storage runtime permission.
    @JavascriptInterface
    fun hasAllFilesAccess(): Boolean =
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
        Environment.isExternalStorageManager()
      } else {
        checkSelfPermission(Manifest.permission.READ_EXTERNAL_STORAGE) == PackageManager.PERMISSION_GRANTED &&
          checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE) == PackageManager.PERMISSION_GRANTED
      }

    @JavascriptInterface
    fun requestAllFilesAccess() {
      runOnUiThread {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) {
          try {
            requestPermissions(
              arrayOf(
                Manifest.permission.READ_EXTERNAL_STORAGE,
                Manifest.permission.WRITE_EXTERNAL_STORAGE
              ), 1001
            )
          } catch (ignored: Exception) {
          }
          return@runOnUiThread
        }
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

    /// JSON array of installed apps: [{pkg,name,size,installed,updated,system,version}]
    @JavascriptInterface
    fun getInstalledApps(): String {
      val out = JSONArray()
      try {
        val pm = packageManager
        @Suppress("DEPRECATION")
        val apps = pm.getInstalledApplications(0)
        for (ai in apps) {
          try {
            val isSystem = (ai.flags and ApplicationInfo.FLAG_SYSTEM) != 0 &&
              (ai.flags and ApplicationInfo.FLAG_UPDATED_SYSTEM_APP) == 0
            var size = File(ai.sourceDir).length()
            ai.splitSourceDirs?.forEach { size += File(it).length() }
            var installed = 0L
            var updated = 0L
            var version = ""
            try {
              @Suppress("DEPRECATION")
              val pi = pm.getPackageInfo(ai.packageName, 0)
              installed = pi.firstInstallTime
              updated = pi.lastUpdateTime
              version = pi.versionName ?: ""
            } catch (ignored: Exception) {
            }
            out.put(
              JSONObject()
                .put("pkg", ai.packageName)
                .put("name", pm.getApplicationLabel(ai).toString())
                .put("size", size)
                .put("installed", installed)
                .put("updated", updated)
                .put("system", isSystem)
                .put("version", version)
            )
          } catch (ignored: Exception) {
          }
        }
      } catch (ignored: Exception) {
      }
      return out.toString()
    }

    @JavascriptInterface
    fun isAppInstalled(pkg: String): Boolean =
      try {
        @Suppress("DEPRECATION")
        packageManager.getPackageInfo(pkg, 0)
        true
      } catch (e: Exception) {
        false
      }

    /// Opens the system uninstall confirmation (works on stock Android, MIUI/HyperOS, One UI...).
    @JavascriptInterface
    fun uninstallApp(pkg: String): Boolean {
      runOnUiThread {
        try {
          @Suppress("DEPRECATION")
          val i = Intent(Intent.ACTION_DELETE, Uri.parse("package:$pkg"))
          i.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
          startActivity(i)
        } catch (e: Exception) {
          try {
            startActivity(
              Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:$pkg"))
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            )
          } catch (ignored: Exception) {
          }
        }
      }
      return true
    }

    @JavascriptInterface
    fun sdkInt(): Int = Build.VERSION.SDK_INT
  }
}
