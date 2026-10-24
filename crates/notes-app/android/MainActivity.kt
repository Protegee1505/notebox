package dev.dioxus.main

import android.app.Activity
import android.app.AlertDialog
import android.content.ClipData
import android.content.ContentProvider
import android.content.ContentValues
import android.content.Intent
import android.database.Cursor
import android.database.MatrixCursor
import android.media.MediaStore
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import android.system.Os
import android.view.KeyEvent
import android.webkit.JavascriptInterface
import android.webkit.WebView
import android.widget.Toast
import org.json.JSONObject
import java.io.File
import java.io.InputStream
import java.util.concurrent.ConcurrentLinkedQueue
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Dioxus generates `Logger.kt` with an unqualified `BuildConfig` reference.
 * Its own template activity provides this alias; a custom activity replaces
 * that file, so the alias has to be restored here.
 */
typealias BuildConfig = app.notebox.BuildConfig

/**
 * The Android shell around the Dioxus WebView.
 *
 * Android's Storage Access Framework and photo picker are exposed to Rust as
 * the `NoteboxAndroid` JavaScript interface. Rust never receives a raw content
 * URI: every picked file is copied into the app cache, and every export is
 * copied from the cache to the document the user chose. That keeps the
 * WebView bridge small and means no file permission is held after the picker
 * closes.
 *
 * The camera writes through [CameraOutputProvider] instead of a real content
 * URI, because a file provider needs a `res/xml` path resource and Dioxus does
 * not copy resource files into the generated Android project.
 */
class MainActivity : WryActivity() {
    private var webView: WebView? = null
    private val results = ConcurrentLinkedQueue<String>()
    private val busy = AtomicBoolean(false)

    private var pendingAction: String? = null
    private var pendingName: String? = null
    private var pendingSource: String? = null
    private var pendingCamera: String? = null

    companion object {
        private const val REQUEST_OPEN_IMAGE = 4101
        private const val REQUEST_OPEN_BACKUP = 4102
        private const val REQUEST_CREATE_FILE = 4103
        private const val REQUEST_CAPTURE_IMAGE = 4104

        /** Matches `images::MAX_INPUT_BYTES`; images are re-encoded after this. */
        private const val MAX_IMAGE_BYTES = 25L * 1024L * 1024L
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
                if (action == "backup") {
                    pendingAction = "backup"
                    try {
                        startActivityForResult(backupIntent(), REQUEST_OPEN_BACKUP)
                    } catch (error: Exception) {
                        fail(error)
                    }
                } else {
                    pendingAction = "image"
                    chooseImageSource()
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

    /**
     * The photo action asks the user where the image comes from. Keeping the
     * choice native means the Rust side still only calls `pick("image")`.
     */
    private fun chooseImageSource() {
        AlertDialog.Builder(this)
            .setTitle("Add an image")
            .setItems(arrayOf("Take a photo", "Choose from gallery")) { _, which ->
                try {
                    if (which == 0) launchCamera() else launchImagePicker()
                } catch (error: Exception) {
                    fail(error)
                }
            }
            .setOnCancelListener { cancelAction() }
            .show()
    }

    private fun launchImagePicker() {
        // The system photo picker never needs a storage permission and only
        // shows images. It exists on Android 13+; older releases and stripped
        // AOSP builds fall back to the document picker.
        val picker = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            Intent(MediaStore.ACTION_PICK_IMAGES).apply { type = "image/*" }
        } else {
            imageDocumentIntent()
        }
        val intent = if (picker.resolveActivity(packageManager) != null) {
            picker
        } else {
            imageDocumentIntent()
        }
        startActivityForResult(intent, REQUEST_OPEN_IMAGE)
    }

    private fun launchCamera() {
        val directory = File(cacheDir, "camera")
        if (!directory.isDirectory && !directory.mkdirs()) {
            throw IllegalStateException("Cannot create the camera cache directory")
        }
        val name = "photo-${System.currentTimeMillis()}.jpg"
        val file = File(directory, name)
        val output = cameraUri(name)
        val intent = Intent(MediaStore.ACTION_IMAGE_CAPTURE).apply {
            putExtra(MediaStore.EXTRA_OUTPUT, output)
            // The grant is per-URI, so no camera permission is involved and
            // the rest of the cache stays private to the app.
            clipData = ClipData.newRawUri("photo", output)
            addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        if (intent.resolveActivity(packageManager) == null) {
            Toast.makeText(this, "No camera app is installed", Toast.LENGTH_SHORT).show()
            launchImagePicker()
            return
        }
        pendingCamera = file.absolutePath
        startActivityForResult(intent, REQUEST_CAPTURE_IMAGE)
    }

    private fun backupIntent() = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
        addCategory(Intent.CATEGORY_OPENABLE)
        // Do not narrow by MIME: some file managers report Notebox's zip as
        // application/octet-stream.
        type = "*/*"
    }

    private fun imageDocumentIntent() = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
        addCategory(Intent.CATEGORY_OPENABLE)
        type = "image/*"
    }

    private fun cameraUri(name: String): Uri =
        Uri.parse("content://${BuildConfig.APPLICATION_ID}.cameraout/photo/$name")

    @Suppress("DEPRECATION")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode !in intArrayOf(
                REQUEST_OPEN_IMAGE,
                REQUEST_OPEN_BACKUP,
                REQUEST_CREATE_FILE,
                REQUEST_CAPTURE_IMAGE,
            )
        ) {
            return
        }

        if (resultCode != Activity.RESULT_OK) {
            cancelAction()
            return
        }

        val action = pendingAction ?: "file"
        try {
            when (requestCode) {
                REQUEST_CAPTURE_IMAGE -> {
                    val path = pendingCamera
                        ?: throw IllegalStateException("The camera returned no file")
                    val file = File(path)
                    if (!file.isFile || file.length() == 0L) {
                        throw IllegalStateException("The camera returned an empty photo")
                    }
                    if (file.length() > MAX_IMAGE_BYTES) {
                        throw IllegalStateException("That photo is larger than 25 MiB")
                    }
                    queueResult(
                        JSONObject()
                            .put("kind", "image")
                            .put("path", path)
                            .put("name", file.name),
                    )
                    finishAction()
                }

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
                    val source = cacheFile(sourcePath)
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
        // A backup can be as large as the database; only images are capped so
        // a huge video cannot fill the cache before Rust rejects it.
        val limit = if (action == "backup") null else MAX_IMAGE_BYTES
        val input = contentResolver.openInputStream(uri)
            ?: throw IllegalStateException("The chosen file cannot be read")
        try {
            input.use { source -> copyWithLimit(source, file, limit) }
        } catch (error: Exception) {
            file.delete()
            throw error
        }
        return file to displayName
    }

    private fun copyWithLimit(input: InputStream, target: File, limit: Long?) {
        var total = 0L
        val buffer = ByteArray(64 * 1024)
        target.outputStream().use { output ->
            while (true) {
                val read = input.read(buffer)
                if (read < 0) break
                total += read
                if (limit != null && total > limit) {
                    throw IllegalStateException("That image is larger than 25 MiB")
                }
                output.write(buffer, 0, read)
            }
        }
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

    /**
     * Resolve an export source. Only files inside the cache may be copied to a
     * user-chosen document, so a compromised page cannot read private data.
     */
    private fun cacheFile(path: String): File {
        val root = cacheDir.canonicalFile
        val file = File(path).canonicalFile
        val inside = file == root || file.path.startsWith(root.path + File.separator)
        if (!inside) throw SecurityException("Only cached files can be exported")
        return file
    }

    private fun cancelAction() {
        val action = pendingAction ?: "file"
        deletePendingSource()
        deletePendingCamera()
        finishAction()
        queueResult(JSONObject().put("kind", "cancel").put("action", action))
    }

    private fun fail(error: Exception) {
        val message = error.message ?: error.javaClass.simpleName
        deletePendingSource()
        deletePendingCamera()
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
        pendingCamera = null
    }

    private fun deletePendingSource() {
        pendingSource?.let { path -> runCatching { File(path).delete() } }
    }

    private fun deletePendingCamera() {
        pendingCamera?.let { path -> runCatching { File(path).delete() } }
    }

    private fun cleanOldCacheFiles() {
        val cutoff = System.currentTimeMillis() - 24 * 60 * 60 * 1000L
        for (root in listOf(cacheDir, File(cacheDir, "camera"))) {
            root.listFiles()?.forEach { file ->
                val ours = file.isFile &&
                    (file.name.startsWith("notebox-") || file.name.startsWith("photo-"))
                if (ours && file.lastModified() < cutoff) {
                    runCatching { file.delete() }
                }
            }
        }
    }
}

/**
 * Serves one camera output file at a time from `cacheDir/camera`.
 *
 * `ACTION_IMAGE_CAPTURE` needs a writable URI. A `FileProvider` would need a
 * `res/xml` path resource that Dioxus does not package, so this provider is
 * declared directly in the manifest and only resolves names inside the camera
 * directory. The camera app only reaches the single URI it was granted.
 */
class CameraOutputProvider : ContentProvider() {
    override fun onCreate(): Boolean = true

    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor {
        val file = cameraFile(uri)
        val flags = if (mode.contains('w')) {
            ParcelFileDescriptor.MODE_WRITE_ONLY or
                ParcelFileDescriptor.MODE_CREATE or
                ParcelFileDescriptor.MODE_TRUNCATE
        } else {
            ParcelFileDescriptor.MODE_READ_ONLY
        }
        return ParcelFileDescriptor.open(file, flags)
    }

    override fun query(
        uri: Uri,
        projection: Array<out String>?,
        selection: String?,
        selectionArgs: Array<out String>?,
        sortOrder: String?,
    ): Cursor {
        val file = cameraFile(uri)
        val columns = projection
            ?: arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE)
        val row = columns.map { column ->
            when (column) {
                OpenableColumns.DISPLAY_NAME -> file.name
                OpenableColumns.SIZE -> file.length()
                else -> null
            }
        }
        return MatrixCursor(columns, 1).apply { addRow(row) }
    }

    override fun getType(uri: Uri): String = "image/jpeg"

    override fun insert(uri: Uri, values: ContentValues?): Uri? = null

    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String>?): Int = 0

    override fun update(
        uri: Uri,
        values: ContentValues?,
        selection: String?,
        selectionArgs: Array<out String>?,
    ): Int = 0

    private fun cameraFile(uri: Uri): File {
        val name = uri.lastPathSegment ?: throw IllegalArgumentException("Missing camera file name")
        val root = File(requireNotNull(context).cacheDir, "camera").canonicalFile
        val file = File(root, name).canonicalFile
        if (file.parentFile != root) throw SecurityException("Camera path escapes the cache")
        if (!file.isFile) throw java.io.FileNotFoundException("No such camera file")
        return file
    }
}
