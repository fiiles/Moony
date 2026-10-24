// moony-keychain, Android half. UNVERIFIED ON ANDROID: written without an Android SDK on the
// build machine; compile and exercise it on a device before relying on it (plan Phase 7).
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
import android.content.pm.PackageManager
import android.content.res.Configuration
import android.graphics.Color
import android.os.Build
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
import java.io.IOException
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

@TauriPlugin
class KeychainPlugin(private val activity: Activity) : Plugin(activity) {
    // UI thread only.
    private var privacyScreenEnabled = true
    private var privacyOverlay: View? = null

    override fun load(webView: WebView) {
        super.load(webView)
        activity.runOnUiThread { applyRecentsScreenshotPolicy() }
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
     * Keeping the app data out of device backups is a no-op here for now: on Android it is not a
     * per-folder flag but `android:allowBackup="false"` (or `<full-backup-content>` and
     * `<data-extraction-rules>`) in the app manifest, which does not exist until `tauri android
     * init` (plan Phase 7). Answers `excluded: false`, so the Rust side logs that nothing is
     * excluded instead of claiming it. (The keystore blobs already live in `noBackupFilesDir`.)
     */
    @Command
    fun excludeFromBackup(invoke: Invoke) {
        guarded(invoke) {
            invoke.resolve(JSObject().apply { put("excluded", false) })
        }
    }

    /**
     * Dark status bar icons over the light theme, light icons over the dark one. UNVERIFIED ON
     * ANDROID, like the rest of this file: exercise it on a device.
     */
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
