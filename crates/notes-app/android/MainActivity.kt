package dev.dioxus.main

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.OpenableColumns
import android.system.Os
import android.view.KeyEvent
import android.webkit.JavascriptInterface
import android.webkit.WebView
import org.json.JSONObject
import java.io.File
import java.util.concurrent.ConcurrentLinkedQueue
import java.util.concurrent.atomic.AtomicBoolean

/**
 * The Android shell around the Dioxus WebView.
 *
 * Android's Storage Access Framework is exposed to Rust as the
 * `NoteboxAndroid` JavaScript interface. Rust never receives a raw content
 * URI: every picked file is copied into the app cache, and every export is
 * copied from the cache to the document the user chose. That keeps the
 * WebView bridge small and means no file permission is held after the picker
 * closes.
 */
class MainActivity : WryActivity() {
    private var webView: WebView? = null
    private val results = ConcurrentLinkedQueue<String>()
    private val busy = AtomicBoolean(false)

    private var pendingAction: String? = null
    private var pendingName: String? = null
    private var pendingSource: String? = null

    companion object {
        private const val REQUEST_OPEN_IMAGE = 4101
        private const val REQUEST_OPEN_BACKUP = 4102
        private const val REQUEST_CREATE_FILE = 4103
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        // Wry starts Rust in super.onCreate. The environment is the only
        // path channel that does not need a JNI callback or unsafe code.
        try {
            Os.setenv("NOTEBOX_DATA_DIR", filesDir.absolutePath, true)
            Os.setenv("NOTEBOX_CACHE_DIR", cacheDir.absolutePath, true)
        } catch (_: Exception) {
            // storage.rs falls back to the platform defaults and reports the
            // path it actually opened.
        }
        cleanOldCacheFiles()
        super.onCreate(savedInstanceState)
    }

    override fun onWebViewCreate(webView: WebView) {
        this.webView = webView
        webView.addJavascriptInterface(Bridge(), "NoteboxAndroid")
    }

    override fun onPause() {
        // Best-effort save before Android may stop the process. Rust also
        // autosaves on its normal ticker; this closes the last few hundred ms.
        try {
            webView?.evaluateJavascript(
                "window.dispatchEvent(new Event('notebox-pause'))",
                null,
            )
        } catch (_: Exception) {
            // The WebView may already be gone during shutdown.
        }
        super.onPause()
    }

    @Suppress("DEPRECATION")
    override fun onBackPressed() {
        handleBack()
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent?): Boolean {
        if (keyCode == KeyEvent.KEYCODE_BACK) {
            handleBack()
            return true
        }
        return super.onKeyDown(keyCode, event)
    }

    private fun handleBack() {
        val web = webView
        if (web == null) {
            finish()
            return
        }
        try {
            web.evaluateJavascript(
                "(window.__noteboxHandleBack && window.__noteboxHandleBack()) === true",
            ) { result ->
                if (result != "true") finish()
            }
        } catch (_: Exception) {
            finish()
        }
    }

    inner class Bridge {
        @JavascriptInterface
        fun pick(action: String) {
            if (!busy.compareAndSet(false, true)) return
            runOnUiThread {
                pendingAction = if (action == "backup") "backup" else "image"
                val intent = if (action == "backup") {
                    Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                        addCategory(Intent.CATEGORY_OPENABLE)
                        // Do not narrow by MIME: some file managers report
                        // Notebox's zip as application/octet-stream.
                        type = "*/*"
                    }
                } else {
                    Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                        addCategory(Intent.CATEGORY_OPENABLE)
                        type = "image/*"
                    }
                }
                try {
                    startActivityForResult(
                        intent,
                        if (action == "backup") REQUEST_OPEN_BACKUP else REQUEST_OPEN_IMAGE,
                    )
                } catch (error: Exception) {
                    fail(error)
                }
            }
        }

        @JavascriptInterface
        fun save(sourcePath: String, name: String, mime: String, action: String) {
            if (!busy.compareAndSet(false, true)) return
            runOnUiThread {
                pendingAction = action
                pendingName = name
                pendingSource = sourcePath
                val intent = Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
                    addCategory(Intent.CATEGORY_OPENABLE)
                    type = mime.ifBlank { "application/octet-stream" }
                    putExtra(Intent.EXTRA_TITLE, name.ifBlank { "notebox-export.zip" })
                }
                try {
                    startActivityForResult(intent, REQUEST_CREATE_FILE)
                } catch (error: Exception) {
                    fail(error)
                }
            }
        }

        @JavascriptInterface
        fun takeResult(): String? = results.poll()
    }

    @Suppress("DEPRECATION")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode !in intArrayOf(
                REQUEST_OPEN_IMAGE,
                REQUEST_OPEN_BACKUP,
                REQUEST_CREATE_FILE,
            )
        ) {
            return
        }

        val action = pendingAction ?: "file"
        if (resultCode != Activity.RESULT_OK) {
            deletePendingSource()
            finishAction()
            queueResult(JSONObject().put("kind", "cancel").put("action", action))
            return
        }

        try {
            when (requestCode) {
                REQUEST_OPEN_IMAGE, REQUEST_OPEN_BACKUP -> {
                    val uri = data?.data
                        ?: throw IllegalStateException("The picker returned no file")
                    val (file, displayName) = copyToCache(uri, action)
                    val kind = if (action == "backup") "backup" else "image"
                    queueResult(
                        JSONObject()
                            .put("kind", kind)
                            .put("path", file.absolutePath)
                            .put("name", displayName),
                    )
                    finishAction()
                }

                REQUEST_CREATE_FILE -> {
                    val target = data?.data
                        ?: throw IllegalStateException("The save dialog returned no location")
                    val sourcePath = pendingSource
                        ?: throw IllegalStateException("The export file is missing")
                    val source = File(sourcePath)
                    val output = contentResolver.openOutputStream(target)
                        ?: throw IllegalStateException("The chosen location cannot be written")
                    output.use { out ->
                        source.inputStream().use { input -> input.copyTo(out) }
                    }
                    source.delete()
                    queueResult(
                        JSONObject()
                            .put("kind", "saved")
                            .put("name", pendingName ?: source.name)
                            .put("action", action),
                    )
                    finishAction()
                }
            }
        } catch (error: Exception) {
            fail(error)
        }
    }

    private fun copyToCache(uri: Uri, action: String): Pair<File, String> {
        val displayName = displayName(uri)
        val safe = displayName
            .replace(Regex("[^A-Za-z0-9._-]"), "_")
            .take(80)
            .ifBlank { action }
        val file = File(
            cacheDir,
            "notebox-$action-${System.currentTimeMillis()}-$safe",
        )
        val input = contentResolver.openInputStream(uri)
            ?: throw IllegalStateException("The chosen file cannot be read")
        input.use { source ->
            file.outputStream().use { target -> source.copyTo(target) }
        }
        return file to displayName
    }

    private fun displayName(uri: Uri): String {
        var name = "file"
        try {
            contentResolver.query(
                uri,
                arrayOf(OpenableColumns.DISPLAY_NAME),
                null,
                null,
                null,
            )?.use { cursor ->
                if (cursor.moveToFirst()) {
                    val index = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                    if (index >= 0) name = cursor.getString(index) ?: name
                }
            }
        } catch (_: Exception) {
            // The name is only a label; the copied file still works.
        }
        return name.ifBlank { "file" }
    }

    private fun fail(error: Exception) {
        val message = error.message ?: error.javaClass.simpleName
        deletePendingSource()
        queueResult(JSONObject().put("kind", "error").put("message", message))
        finishAction()
    }

    private fun queueResult(result: JSONObject) {
        results.offer(result.toString())
    }

    private fun finishAction() {
        busy.set(false)
        pendingAction = null
        pendingName = null
        pendingSource = null
    }

    private fun deletePendingSource() {
        pendingSource?.let { path -> runCatching { File(path).delete() } }
    }

    private fun cleanOldCacheFiles() {
        val cutoff = System.currentTimeMillis() - 24 * 60 * 60 * 1000L
        cacheDir.listFiles()?.forEach { file ->
            if (file.name.startsWith("notebox-") && file.lastModified() < cutoff) {
                runCatching { file.delete() }
            }
        }
    }
}
