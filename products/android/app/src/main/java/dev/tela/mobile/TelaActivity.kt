package dev.tela.mobile

import android.os.Bundle
import android.text.Editable
import android.text.InputType
import android.text.TextWatcher
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.widget.EditText
import androidx.core.view.WindowInsetsCompat
import com.google.androidgamesdk.GameActivity

/**
 * Android owns the Activity, IME, and system Back. The Rust GameActivity main loop owns the
 * Vulkan surface and translates the resulting platform-neutral input events for the guest.
 */
class TelaActivity : GameActivity() {
    companion object {
        private const val BACK_NOT_HANDLED = 0
        private const val BACK_BLURRED_TEXT_INPUT = 1
        private const val BACK_DISPATCHED_TO_GUEST = 2

        init {
            System.loadLibrary("main")
        }
    }

    private lateinit var textInput: EditText
    private var applyingGuestValue = false
    private var appliedTopSafeInset = -1

    private val textSynchronizer = object : Runnable {
        override fun run() {
            if (nativeConsumeFinishRequested()) {
                finish()
                return
            }
            synchronizeTextInput()
            textInput.postDelayed(this, 16L)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        // GameActivity starts Rust while its superclass initializes. Configure before that point
        // so android_main sees the exact URL Gradle injected into this build.
        nativeConfigureBundleIndex(BuildConfig.TELA_BUNDLE_INDEX)
        super.onCreate(savedInstanceState)
        window.setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_NOTHING)
        installTextInput()
        textInput.post(textSynchronizer)
    }

    override fun onDestroy() {
        if (::textInput.isInitialized) {
            textInput.removeCallbacks(textSynchronizer)
        }
        super.onDestroy()
    }

    override fun onApplyWindowInsets(view: View, insets: WindowInsetsCompat): WindowInsetsCompat {
        // Android 15+ lays target-SDK-35+ apps edge-to-edge. Keep the native SurfaceView below
        // the status bar and any display cutout, while leaving GameActivity's IME bridge intact.
        val topInset = insets.getInsets(
            WindowInsetsCompat.Type.statusBars() or WindowInsetsCompat.Type.displayCutout(),
        ).top
        applyTopSafeInset(topInset)
        return super.onApplyWindowInsets(view, insets)
    }

    @Deprecated("Delegates to the target host until predictive Back is adopted by GameActivity.")
    override fun onBackPressed() {
        when (nativeSystemBack()) {
            BACK_BLURRED_TEXT_INPUT -> hideTextInputImmediately()
            BACK_DISPATCHED_TO_GUEST -> Unit
            BACK_NOT_HANDLED -> super.onBackPressed()
        }
    }

    private fun installTextInput() {
        textInput = EditText(this).apply {
            alpha = 0.01f
            background = null
            isCursorVisible = false
            gravity = Gravity.TOP or Gravity.START
            imeOptions = EditorInfo.IME_ACTION_DONE
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES
            isSingleLine = true
            setTextColor(0x00000000)
            setHintTextColor(0x00000000)
            setSelectAllOnFocus(false)
            addTextChangedListener(object : TextWatcher {
                override fun beforeTextChanged(
                    sequence: CharSequence?,
                    start: Int,
                    count: Int,
                    after: Int,
                ) {
                    if (!applyingGuestValue) {
                        nativeCompositionStart()
                    }
                }

                override fun onTextChanged(
                    sequence: CharSequence?,
                    start: Int,
                    before: Int,
                    count: Int,
                ) {
                    if (!applyingGuestValue) {
                        nativeSetInputValue(sequence?.toString().orEmpty())
                    }
                }

                override fun afterTextChanged(editable: Editable?) {
                    if (!applyingGuestValue) {
                        nativeCompositionEnd()
                    }
                }
            })
            setOnFocusChangeListener { _, focused ->
                if (!applyingGuestValue) {
                    if (focused) nativeInputFocus() else nativeInputBlur()
                }
            }
            setOnEditorActionListener { _, actionId, _ ->
                if (actionId == EditorInfo.IME_ACTION_DONE) {
                    nativeInputEnter()
                    true
                } else {
                    false
                }
            }
        }
        addContentView(textInput, ViewGroup.LayoutParams(1, 1))
    }

    private fun synchronizeTextInput() {
        val shouldFocus = nativeInputFocused()
        val desiredValue = nativeInputValue()
        if (textInput.text.toString() != desiredValue) {
            applyingGuestValue = true
            textInput.setText(desiredValue)
            textInput.setSelection(textInput.text.length)
            applyingGuestValue = false
        }
        if (shouldFocus && !textInput.hasFocus()) {
            textInput.requestFocus()
            textInput.post {
                val imm = getSystemService(InputMethodManager::class.java)
                imm.showSoftInput(textInput, InputMethodManager.SHOW_IMPLICIT)
            }
        } else if (!shouldFocus && textInput.hasFocus()) {
            hideTextInputImmediately()
        }
    }

    private fun hideTextInputImmediately() {
        val imm = getSystemService(InputMethodManager::class.java)
        imm.hideSoftInputFromWindow(textInput.windowToken, 0)
        textInput.clearFocus()
    }

    private fun applyTopSafeInset(topInset: Int) {
        if (topInset == appliedTopSafeInset) return
        val layout = mSurfaceView.layoutParams as? ViewGroup.MarginLayoutParams ?: return
        if (layout.topMargin != topInset) {
            layout.topMargin = topInset
            mSurfaceView.layoutParams = layout
        }
        appliedTopSafeInset = topInset
    }

    private external fun nativeConfigureBundleIndex(value: String)
    private external fun nativeInputFocused(): Boolean
    private external fun nativeInputValue(): String
    private external fun nativeSetInputValue(value: String): Boolean
    private external fun nativeInputFocus(): Boolean
    private external fun nativeInputBlur(): Boolean
    private external fun nativeInputEnter(): Boolean
    private external fun nativeCompositionStart(): Boolean
    private external fun nativeCompositionEnd(): Boolean
    private external fun nativeSystemBack(): Int
    private external fun nativeConsumeFinishRequested(): Boolean
}
