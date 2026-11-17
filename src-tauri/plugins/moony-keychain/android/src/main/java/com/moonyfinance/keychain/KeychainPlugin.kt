// moony-keychain, Android half. Built and run on the emulator since 2026-11-17 (privacy screen,
// status bar, backup state, the content:// copies); the biometric store and retrieve are not yet
// exercised with an enrolled fingerprint (docs/plans/2026-11-17-android-app.md).
//
// One secret per key: an AndroidKeyStore AES-256-GCM key per entry that needs strong biometry
// for every use and dies when the biometric enrolment changes, and the ciphertext (IV first) in
// app-private, never-backed-up storage. Every use of the key, encrypting included, goes through
// a BiometricPrompt with a CryptoObject.
//
// Also the privacy screen: an opaque view in the app's canvas colour over the window while the
// activity is paused, and on Android 13+ no recents screenshot at all. No FLAG_SECURE, so the
// person can still take screenshots.
//
// And the status bar style: dark or light icons to match the app's theme (`setStatusBarStyle`).

package com.moonyfinance.keychain

import android.app.Activity
import android.content.pm.ApplicationInfo
import android.content.pm.PackageManager
import android.content.res.Configuration
import android.graphics.Color
import android.net.Uri
import android.os.Build
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import android.util.Log
import android.util.TypedValue
import android.view.View
import android.view.ViewGroup
import android.webkit.WebView
import androidx.appcompat.app.AppCompatActivity
import androidx.biometric.BiometricManager
import androidx.biometric.BiometricManager.Authenticators.BIOMETRIC_STRONG
import androidx.biometric.BiometricPrompt
import androidx.core.content.ContextCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.fragment.app.FragmentActivity
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.json.JSONObject
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

private const val TAG = "MoonyKeychain"
private const val KEYSTORE = "AndroidKeyStore"
private const val TRANSFORMATION = "AES/GCM/NoPadding"
private const val KEY_ALIAS_PREFIX = "com.moonyfinance.mobile.keychain/"
private const val STORE_DIR = "moony-keychain"
private const val GCM_TAG_BITS = 128
private const val GCM_IV_BYTES = 12
// Where a copy from a content:// URI is written before it moves into its folder (app cache).
private const val INCOMING_DIR = "moony-incoming"
// The longest file name a copy gets, in UTF-8 bytes.
private const val MAX_NAME_BYTES = 200

// Rejection codes. The Rust side parses them with `KeychainError::from_rejection`.
private const val CODE_INVALIDATED = "invalidated"
private const val CODE_CANCELLED = "cancelled"
private const val CODE_UNAVAILABLE = "unavailable"
private const val CODE_FAILED = "failed"

@InvokeArg
class StoreArgs {
    lateinit var key: String
    lateinit var value: String
    // The prompt title ("Turn on biometric unlock"); the app label when missing.
    var reason: String? = null
}

@InvokeArg
class RetrieveArgs {
    lateinit var key: String
    lateinit var reason: String
}

@InvokeArg
class RemoveArgs {
    lateinit var key: String
}

@InvokeArg
class PrivacyScreenArgs {
    var enabled: Boolean = true
}

@InvokeArg
class StatusBarStyleArgs {
    // The app's resolved theme: "light" or "dark".
    lateinit var theme: String
}

@InvokeArg
class CopyFromUriArgs {
    // A content:// URI the system handed over (a picked file, a shared or opened statement).
    lateinit var uri: String
    // The app-private folder the copy goes into (Rust picks it; the inbox folder).
    lateinit var dir: String
    // The largest copy accepted, in bytes; 0 for no limit (a picked backup can be large).
    var maxBytes: Long = 0
}

@InvokeArg
class CopyToUriArgs {
    // A staged export inside the app (Rust checked it is one).
    lateinit var path: String
    // The content:// URI of the document the person created in the save picker.
    lateinit var uri: String
}

@TauriPlugin
class KeychainPlugin(private val activity: Activity) : Plugin(activity) {
    // UI thread only.
    private var privacyScreenEnabled = true
    private var privacyOverlay: View? = null

    override fun load(webView: WebView) {
        super.load(webView)
        activity.runOnUiThread { applyRecentsScreenshotPolicy() }
        // Before any copy of this process starts: what an earlier one left half-written
        Thread {
            File(activity.cacheDir, INCOMING_DIR).listFiles()?.forEach { it.delete() }
        }.start()
    }

    // ---- Commands ----

    @Command
    fun status(invoke: Invoke) {
        guarded(invoke) {
            val result = JSObject()
            result.put(
                "available",
                BiometricManager.from(activity).canAuthenticate(BIOMETRIC_STRONG) ==
                    BiometricManager.BIOMETRIC_SUCCESS
            )
            result.put("biometryType", biometryType())
            invoke.resolve(result)
        }
    }

    @Command
    fun store(invoke: Invoke) {
        guarded(invoke) {
            val args = invoke.parseArgs(StoreArgs::class.java)
            val key = args.key
            val plaintext = args.value.toByteArray(Charsets.UTF_8)
            val title = args.reason?.takeIf { it.isNotBlank() } ?: appLabel()

            if (BiometricManager.from(activity).canAuthenticate(BIOMETRIC_STRONG) !=
                BiometricManager.BIOMETRIC_SUCCESS
            ) {
                reject(invoke, CODE_UNAVAILABLE, "Strong biometry is unavailable")
                return@guarded
            }

            // Replace, never update: the new key binds to today's enrolment. A cancelled prompt
            // leaves nothing stored, like on iOS after a failed add.
            removeEntry(key)
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.ENCRYPT_MODE, createKey(keyAlias(key)))

            prompt(invoke, title, cipher) { authenticated ->
                val iv = authenticated.iv
                if (iv.size != GCM_IV_BYTES) {
                    throw IOException("Unexpected GCM IV length ${iv.size}")
                }
                writeBlob(key, iv + authenticated.doFinal(plaintext))
                invoke.resolve()
            }
        }
    }

    @Command
    fun retrieve(invoke: Invoke) {
        guarded(invoke) {
            val args = invoke.parseArgs(RetrieveArgs::class.java)
            val key = args.key
            val reason = args.reason

            val blob = readBlob(key)
            if (blob == null) {
                // Nothing stored under this key: answer without prompting.
                invoke.resolve(JSObject().apply { put("value", JSONObject.NULL) })
                return@guarded
            }
            if (blob.size <= GCM_IV_BYTES) {
                reject(invoke, CODE_FAILED, "The stored secret is truncated")
                return@guarded
            }

            // Removing the lock screen deletes auth-bound keys outright.
            val secretKey = loadKey(keyAlias(key))
            if (secretKey == null) {
                reject(invoke, CODE_INVALIDATED, "The key behind the secret no longer exists")
                return@guarded
            }

            val cipher = Cipher.getInstance(TRANSFORMATION)
            try {
                cipher.init(
                    Cipher.DECRYPT_MODE,
                    secretKey,
                    GCMParameterSpec(GCM_TAG_BITS, blob, 0, GCM_IV_BYTES)
                )
            } catch (e: KeyPermanentlyInvalidatedException) {
                reject(invoke, CODE_INVALIDATED, "The biometric enrolment changed since the secret was stored")
                return@guarded
            }

            when (BiometricManager.from(activity).canAuthenticate(BIOMETRIC_STRONG)) {
                BiometricManager.BIOMETRIC_SUCCESS -> Unit
                BiometricManager.BIOMETRIC_ERROR_NONE_ENROLLED -> {
                    reject(invoke, CODE_INVALIDATED, "Every biometric enrolment was removed")
                    return@guarded
                }
                else -> {
                    reject(invoke, CODE_UNAVAILABLE, "Strong biometry is unavailable")
                    return@guarded
                }
            }

            prompt(invoke, reason, cipher) { authenticated ->
                val plaintext = authenticated.doFinal(blob, GCM_IV_BYTES, blob.size - GCM_IV_BYTES)
                invoke.resolve(JSObject().apply { put("value", String(plaintext, Charsets.UTF_8)) })
            }
        }
    }

    @Command
    fun remove(invoke: Invoke) {
        guarded(invoke) {
            val args = invoke.parseArgs(RemoveArgs::class.java)
            if (removeEntry(args.key)) {
                invoke.resolve()
            } else {
                reject(invoke, CODE_FAILED, "Could not delete the stored secret")
            }
        }
    }

    @Command
    fun setPrivacyScreen(invoke: Invoke) {
        guarded(invoke) {
            val args = invoke.parseArgs(PrivacyScreenArgs::class.java)
            val enabled = args.enabled
            activity.runOnUiThread {
                privacyScreenEnabled = enabled
                applyRecentsScreenshotPolicy()
                if (!enabled) {
                    hidePrivacyScreen()
                }
            }
            // Answer at once: a Rust caller blocks on the answer.
            invoke.resolve()
        }
    }

    /**
     * On Android keeping the app data out of device backups is not a per-folder flag but the app
     * manifest: `android:allowBackup="false"` plus `fullBackupContent` and `dataExtractionRules`
     * that exclude every domain (gen/android). Nothing to set per path, so this answers whether the
     * manifest turned backup off: `excluded: false` only if a build ever drops that attribute, and
     * the Rust side then logs it instead of claiming the exclusion.
     */
    @Command
    fun excludeFromBackup(invoke: Invoke) {
        guarded(invoke) {
            val backupAllowed =
                (activity.applicationInfo.flags and ApplicationInfo.FLAG_ALLOW_BACKUP) != 0
            invoke.resolve(JSObject().apply { put("excluded", !backupAllowed) })
        }
    }

    // ---- Files from and to content:// URIs ----
    //
    // A copy is written into `INCOMING_DIR` next to the target folder and moved into it only when
    // complete, so the inbox never holds a half-written file (its startup sweep removes every file
    // there). What a killed process left in `INCOMING_DIR` is removed when the plugin loads.
    //
    // Android hands picked, shared and opened files over as content:// URIs that only the
    // ContentResolver reads, and the save picker returns the URI of an empty document. Rust works
    // with paths, so these two copy between a URI and a file inside the app. Rust-only (not in the
    // plugin's ACL): Rust decides which folder a copy lands in and which staged file may leave.
    // The copies run off the UI thread (a backup archive can be large). Neither logs the URI or
    // the name: they name the person's bank files.

    /** Copies the document at `uri` into `dir` under its display name; answers the new path. */
    @Command
    fun copyFromUri(invoke: Invoke) {
        guarded(invoke) {
            val args = invoke.parseArgs(CopyFromUriArgs::class.java)
            val uri = Uri.parse(args.uri)
            val dir = File(args.dir)
            val maxBytes = args.maxBytes
            background(invoke) {
                val incoming = File(activity.cacheDir, INCOMING_DIR)
                if (!dir.isDirectory && !dir.mkdirs() || !incoming.isDirectory && !incoming.mkdirs()) {
                    throw IOException("Could not create the inbox folder")
                }
                val partial = File.createTempFile("copy", ".part", incoming)
                try {
                    val input = activity.contentResolver.openInputStream(uri)
                        ?: throw IOException("The provider gave no stream")
                    input.use { source ->
                        FileOutputStream(partial).use { sink -> copyCapped(source, sink, maxBytes) }
                    }
                    val target = reserveUniqueFile(dir, safeFileName(displayName(uri)))
                    if (!partial.renameTo(target)) {
                        target.delete()
                        throw IOException("Could not move the copy into the inbox")
                    }
                    invoke.resolve(JSObject().apply { put("path", target.absolutePath) })
                } finally {
                    partial.delete()
                }
            }
        }
    }

    /** Writes the file at `path` into the document at `uri`, replacing what it held. */
    @Command
    fun copyToUri(invoke: Invoke) {
        guarded(invoke) {
            val args = invoke.parseArgs(CopyToUriArgs::class.java)
            val source = File(args.path)
            val uri = Uri.parse(args.uri)
            background(invoke) {
                try {
                    val output = openForWrite(uri) ?: throw IOException("The provider gave no stream")
                    output.use { sink -> FileInputStream(source).use { it.copyTo(sink) } }
                } catch (e: Throwable) {
                    // The picker has already created the document: a truncated backup or key must
                    // not stay where the person would later trust it. Best effort.
                    try {
                        DocumentsContract.deleteDocument(activity.contentResolver, uri)
                    } catch (_: Throwable) {
                    }
                    throw e
                }
                invoke.resolve()
            }
        }
    }

    /** "wt" truncates; a provider without it (some cloud ones) gets "w" on a fresh document. */
    private fun openForWrite(uri: Uri): OutputStream? =
        try {
            activity.contentResolver.openOutputStream(uri, "wt")
        } catch (e: IllegalArgumentException) {
            activity.contentResolver.openOutputStream(uri, "w")
        } catch (e: UnsupportedOperationException) {
            activity.contentResolver.openOutputStream(uri, "w")
        }

    private fun displayName(uri: Uri): String? =
        try {
            activity.contentResolver
                .query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
                ?.use { cursor ->
                    if (cursor.moveToFirst()) cursor.getString(0) else null
                }
        } catch (e: Exception) {
            null
        } ?: uri.lastPathSegment

    /** Copies `source` into `sink`, failing once more than `maxBytes` (when > 0) came through. */
    private fun copyCapped(source: InputStream, sink: OutputStream, maxBytes: Long) {
        val buffer = ByteArray(64 * 1024)
        var total = 0L
        while (true) {
            val read = source.read(buffer)
            if (read < 0) return
            total += read
            if (maxBytes > 0 && total > maxBytes) throw IOException("The file is too large")
            sink.write(buffer, 0, read)
        }
    }

    /**
     * A plain file name: no folders, no hidden-file dot first, at most 200 UTF-8 bytes (ext4 allows
     * 255, and " (n)" may be added) with the extension kept (the import reads the extension to
     * tell a PDF or a photo from a statement), never cut inside a surrogate pair.
     */
    private fun safeFileName(name: String?): String {
        val base = name
            ?.substringAfterLast('/')
            ?.replace(Regex("[\\u0000-\\u001f\\\\:*?\"<>|]"), "_")
            ?.trim()
            ?.trimStart('.')
            .orEmpty()
        if (base.isEmpty()) return "statement"
        if (utf8Length(base) <= MAX_NAME_BYTES) return base
        val dot = base.lastIndexOf('.')
        val ext = if (dot > 0 && base.length - dot <= 10) base.substring(dot) else ""
        var stem = base.substring(0, base.length - ext.length)
        while (stem.isNotEmpty() && utf8Length(stem + ext) > MAX_NAME_BYTES) {
            stem = stem.dropLast(if (stem.length >= 2 && stem[stem.length - 1].isLowSurrogate()) 2 else 1)
        }
        return stem + ext
    }

    private fun utf8Length(text: String): Int = text.toByteArray(Charsets.UTF_8).size

    /**
     * Creates and answers `name`, or `name (2)`, `name (3)`, ... when an earlier copy still sits in
     * `dir`; `createNewFile` claims the name, so two copies at once never get the same one.
     */
    private fun reserveUniqueFile(dir: File, name: String): File {
        val dot = name.lastIndexOf('.')
        val stem = if (dot > 0) name.substring(0, dot) else name
        val ext = if (dot > 0) name.substring(dot) else ""
        var n = 1
        while (true) {
            val candidate = File(dir, if (n == 1) name else "$stem ($n)$ext")
            if (candidate.createNewFile()) return candidate
            n += 1
        }
    }

    /**
     * Runs `body` on a fresh thread, rejecting with the class name only; any `Throwable`, so the
     * Rust caller blocked on the answer is never left waiting.
     */
    private fun background(invoke: Invoke, body: () -> Unit) {
        Thread {
            try {
                body()
            } catch (e: Throwable) {
                reject(invoke, CODE_FAILED, "File copy failed: ${e.javaClass.simpleName}")
            }
        }.start()
    }

    /** Dark status bar icons over the light theme, light icons over the dark one. */
    @Command
    fun setStatusBarStyle(invoke: Invoke) {
        guarded(invoke) {
            val args = invoke.parseArgs(StatusBarStyleArgs::class.java)
            val light = args.theme == "light"
            activity.runOnUiThread {
                val window = activity.window
                val controller = WindowInsetsControllerCompat(window, window.decorView)
                controller.isAppearanceLightStatusBars = light
            }
            invoke.resolve()
        }
    }

    // ---- Privacy screen ----

    override fun onPause(activity: AppCompatActivity) {
        showPrivacyScreen(activity)
    }

    override fun onResume(activity: AppCompatActivity) {
        hidePrivacyScreen()
    }

    private fun showPrivacyScreen(host: Activity) {
        if (!privacyScreenEnabled || privacyOverlay != null) return
        val decor = host.window?.decorView as? ViewGroup ?: return
        val overlay = View(host).apply {
            setBackgroundColor(canvasColor(host))
            isClickable = true
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
        }
        decor.addView(
            overlay,
            ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
        )
        privacyOverlay = overlay
    }

    private fun hidePrivacyScreen() {
        val overlay = privacyOverlay ?: return
        (overlay.parent as? ViewGroup)?.removeView(overlay)
        privacyOverlay = null
    }

    /**
     * The overlay may land after the recents snapshot on some versions; Android 13+ can skip the
     * snapshot altogether (screenshots stay allowed, unlike FLAG_SECURE).
     */
    private fun applyRecentsScreenshotPolicy() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            activity.setRecentsScreenshotEnabled(!privacyScreenEnabled)
        }
    }

    private fun canvasColor(host: Activity): Int {
        val value = TypedValue()
        val resolved = host.theme.resolveAttribute(android.R.attr.colorBackground, value, true)
        if (resolved &&
            value.type >= TypedValue.TYPE_FIRST_COLOR_INT &&
            value.type <= TypedValue.TYPE_LAST_COLOR_INT
        ) {
            return value.data
        }
        val night = host.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK
        return if (night == Configuration.UI_MODE_NIGHT_YES) Color.BLACK else Color.WHITE
    }

    // ---- Biometric prompt ----

    private fun prompt(invoke: Invoke, title: String, cipher: Cipher, onSuccess: (Cipher) -> Unit) {
        val host = activity as? FragmentActivity
        if (host == null) {
            reject(invoke, CODE_UNAVAILABLE, "The activity cannot host a biometric prompt")
            return
        }
        host.runOnUiThread {
            val callback = object : BiometricPrompt.AuthenticationCallback() {
                override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                    val authenticated = result.cryptoObject?.cipher
                    if (authenticated == null) {
                        reject(invoke, CODE_FAILED, "No cipher after authentication")
                        return
                    }
                    try {
                        onSuccess(authenticated)
                    } catch (e: KeyPermanentlyInvalidatedException) {
                        reject(invoke, CODE_INVALIDATED, "The biometric enrolment changed since the secret was stored")
                    } catch (e: Exception) {
                        reject(invoke, CODE_FAILED, "Keychain operation failed: ${e.javaClass.simpleName}")
                    }
                }

                override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                    reject(invoke, codeForPromptError(errorCode), errString.toString())
                }

                // onAuthenticationFailed: one unrecognised attempt; the prompt stays up.
            }
            val info = BiometricPrompt.PromptInfo.Builder()
                .setTitle(title)
                .setNegativeButtonText(host.getString(android.R.string.cancel))
                .setAllowedAuthenticators(BIOMETRIC_STRONG)
                .setConfirmationRequired(false)
                .build()
            BiometricPrompt(host, ContextCompat.getMainExecutor(host), callback)
                .authenticate(info, BiometricPrompt.CryptoObject(cipher))
        }
    }

    private fun codeForPromptError(errorCode: Int): String = when (errorCode) {
        BiometricPrompt.ERROR_USER_CANCELED,
        BiometricPrompt.ERROR_NEGATIVE_BUTTON,
        BiometricPrompt.ERROR_CANCELED -> CODE_CANCELLED
        BiometricPrompt.ERROR_LOCKOUT,
        BiometricPrompt.ERROR_LOCKOUT_PERMANENT,
        BiometricPrompt.ERROR_HW_UNAVAILABLE,
        BiometricPrompt.ERROR_HW_NOT_PRESENT,
        BiometricPrompt.ERROR_NO_BIOMETRICS,
        BiometricPrompt.ERROR_NO_DEVICE_CREDENTIAL -> CODE_UNAVAILABLE
        else -> CODE_FAILED
    }

    private fun biometryType(): String {
        val pm = activity.packageManager
        val fingerprint = pm.hasSystemFeature(PackageManager.FEATURE_FINGERPRINT)
        val q = Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q
        val face = q && pm.hasSystemFeature(PackageManager.FEATURE_FACE)
        val iris = q && pm.hasSystemFeature(PackageManager.FEATURE_IRIS)
        return when {
            fingerprint && !face && !iris -> "touchId"
            face && !fingerprint && !iris -> "faceId"
            fingerprint || face || iris -> "biometric"
            else -> "none"
        }
    }

    private fun appLabel(): String = activity.applicationInfo.loadLabel(activity.packageManager).toString()

    // ---- Keystore and storage ----

    private fun createKey(alias: String): SecretKey {
        val spec = KeyGenParameterSpec.Builder(
            alias,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .setUserAuthenticationRequired(true)
            .setInvalidatedByBiometricEnrollment(true)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            // Timeout 0: strong biometry for every single use, through the CryptoObject.
            spec.setUserAuthenticationParameters(0, KeyProperties.AUTH_BIOMETRIC_STRONG)
        }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        generator.init(spec.build())
        return generator.generateKey()
    }

    private fun keyStore(): KeyStore = KeyStore.getInstance(KEYSTORE).apply { load(null) }

    private fun loadKey(alias: String): SecretKey? = keyStore().getKey(alias, null) as? SecretKey

    private fun keyAlias(key: String): String = KEY_ALIAS_PREFIX + key

    private fun blobFile(key: String): File {
        val dir = File(activity.noBackupFilesDir, STORE_DIR)
        dir.mkdirs()
        val name = key.toByteArray(Charsets.UTF_8).joinToString("") { "%02x".format(it) }
        return File(dir, "$name.bin")
    }

    private fun readBlob(key: String): ByteArray? {
        val file = blobFile(key)
        return if (file.exists()) file.readBytes() else null
    }

    private fun writeBlob(key: String, bytes: ByteArray) {
        val target = blobFile(key)
        val temp = File(target.parentFile, target.name + ".tmp")
        temp.writeBytes(bytes)
        if (!temp.renameTo(target)) {
            temp.delete()
            throw IOException("Could not move the secret into place")
        }
    }

    /** Deletes the ciphertext and its key; true when neither is left. */
    private fun removeEntry(key: String): Boolean {
        val file = blobFile(key)
        val fileGone = !file.exists() || file.delete()
        val store = keyStore()
        val alias = keyAlias(key)
        if (store.containsAlias(alias)) {
            store.deleteEntry(alias)
        }
        return fileGone
    }

    // ---- Helpers ----

    // The exception class only, never its message: a failed parse of the store arguments could
    // echo the request (the secret) into logcat and the rejection.
    private inline fun guarded(invoke: Invoke, body: () -> Unit) {
        try {
            body()
        } catch (e: Exception) {
            reject(invoke, CODE_FAILED, "Keychain operation failed: ${e.javaClass.simpleName}")
        }
    }

    private fun reject(invoke: Invoke, code: String, message: String) {
        Log.w(TAG, "${invoke.command} rejected ($code): $message")
        invoke.reject(message, code)
    }
}
