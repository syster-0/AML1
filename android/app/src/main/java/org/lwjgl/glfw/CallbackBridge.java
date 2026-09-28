package org.lwjgl.glfw;

import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.Intent;
import android.net.Uri;
import android.util.Log;

/**
 * ART-side input bridge for the game process.
 *
 * <p>This is the app-side counterpart of the GLFW stub classes that run inside
 * the HotSpot JVM: touch/key events produced by Android views are handed to
 * the native bridge ({@code libpojavexec.so}), which queues them for the game
 * thread, and the native side calls back here for clipboard access and grab
 * state changes. It lives in the {@code org.lwjgl.glfw} package because the
 * native bridge resolves it by that exact class name.
 *
 * <p>The native methods are bound through {@code RegisterNatives} when the
 * Rust surface bridge registers the ART VM; they must never be invoked before
 * that point.
 */
public final class CallbackBridge {

    private static final String TAG = "aml-input";

    // Clipboard action ids, matching the native bridge contract.
    public static final int CLIPBOARD_COPY = 2000;
    public static final int CLIPBOARD_PASTE = 2001;
    public static final int CLIPBOARD_OPEN = 2002;

    private static Context appContext;

    /** Latest grab state reported by the game; read by input views. */
    private static volatile boolean grabbing = false;

    /** Set by {@link com.astral.aml.GameActivity} before the game boots. */
    public static void attach(Context context) {
        appContext = context.getApplicationContext();
    }

    // Native input entry points (implemented in libpojavexec.so).
    public static native boolean nativeSendChar(char codepoint);
    public static native boolean nativeSendCharMods(char codepoint, int mods);
    public static native void nativeSendKey(int key, int scancode, int action, int mods);
    public static native void nativeSendCursorPos(float x, float y);
    public static native void nativeSendMouseButton(int button, int action, int mods);
    public static native void nativeSendScroll(double xoffset, double yoffset);
    public static native void nativeSendScreenSize(int width, int height);

    /**
     * Called from the native bridge to access the Android clipboard.
     *
     * @param action one of the {@code CLIPBOARD_*} constants
     * @param src    the text to copy/open (ignored for paste)
     * @return the current clipboard text for paste, otherwise {@code null}
     */
    public static String accessAndroidClipboard(int action, String src) {
        if (appContext == null) {
            return null;
        }
        ClipboardManager manager =
                (ClipboardManager) appContext.getSystemService(Context.CLIPBOARD_SERVICE);
        switch (action) {
            case CLIPBOARD_COPY:
                manager.setPrimaryClip(ClipData.newPlainText("minecraft", src));
                return null;
            case CLIPBOARD_PASTE:
                if (!manager.hasPrimaryClip()) {
                    return null;
                }
                ClipData.Item item = manager.getPrimaryClip().getItemAt(0);
                return item == null || item.getText() == null ? null : item.getText().toString();
            case CLIPBOARD_OPEN:
                Intent intent = new Intent(Intent.ACTION_VIEW, Uri.parse(src))
                        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
                try {
                    appContext.startActivity(intent);
                } catch (Exception e) {
                    Log.w(TAG, "Cannot open URL: " + e.getMessage());
                }
                return null;
            default:
                return null;
        }
    }

    /** Called from the native bridge when the game's grab state changes. */
    public static void onGrabStateChanged(boolean isGrabbing) {
        grabbing = isGrabbing;
        Log.i(TAG, "Grab state: " + isGrabbing);
    }

    /** Called from the native bridge when direct gamepad input is enabled. */
    public static void onDirectInputEnable() {
        Log.i(TAG, "Direct gamepad input enabled");
    }

    public static boolean isGrabbing() {
        return grabbing;
    }

    private CallbackBridge() {
        // Static holder only.
    }

    static {
        // Loaded eagerly here as well; if the native launcher already mapped
        // the library, this is a no-op (the loader increments its refcount).
        System.loadLibrary("pojavexec");
    }
}
