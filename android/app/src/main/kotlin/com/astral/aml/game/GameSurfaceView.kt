package com.astral.aml.game

import android.annotation.SuppressLint
import android.content.Context
import android.view.MotionEvent
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.ViewConfiguration
import org.lwjgl.glfw.CallbackBridge

/**
 * Game-process surface host.
 *
 * Owns a [SurfaceView] and mirrors its surface lifecycle into the native game
 * side. `surfaceDestroyed` is not called destructively: the Rust render thread
 * keeps running and merely detaches, so a recreated surface (rotation, lock
 * screen, Activity recreate) can re-attach without a full teardown.
 *
 * Touch input (Phase 6, minimal) is translated here and handed straight to the
 * pojavexec input bridge through [CallbackBridge]'s registered natives — there
 * is no Rust round-trip on the event path. Two modes, switched by the grab
 * state the game reports through `CallbackBridge.onGrabStateChanged`:
 *
 *  - Menu mode (cursor free): the finger drives an absolute virtual cursor;
 *    a quick tap is a left click, holding past the touch slop drags with the
 *    left button held (list scrolling, sliders).
 *  - Grab mode (in-game, cursor disabled): finger movement feeds relative
 *    deltas to the virtual cursor (unbounded, like GLFW_CURSOR_DISABLED on
 *    desktop); a quick tap is a left click (attack/break).
 */
class GameSurfaceView(context: Context) : SurfaceView(context) {

    // GLFW constants (values fixed by the GLFW API).
    private companion object {
        const val GLFW_MOUSE_BUTTON_LEFT = 0
        const val GLFW_PRESS = 1
        const val GLFW_RELEASE = 0

        /** Cursor pixels travelled per touch pixel in grab mode. */
        const val LOOK_SENSITIVITY = 0.5f
    }

    private val touchSlop = ViewConfiguration.get(context).scaledTouchSlop
    private val tapTimeout = ViewConfiguration.getTapTimeout().toLong()

    // Surface size in pixels; also the GLFW window coordinate space.
    private var surfaceWidth = 0
    private var surfaceHeight = 0

    // Virtual cursor position in window coordinates. Unbounded in grab mode.
    private var cursorX = 0f
    private var cursorY = 0f

    private var activePointerId = MotionEvent.INVALID_POINTER_ID
    private var lastTouchX = 0f
    private var lastTouchY = 0f
    private var downX = 0f
    private var downY = 0f
    private var downTime = 0L
    private var dragging = false

    private var wasGrabbing = false

    init {
        // The holder must be set before the Surface can be drawn to.
        holder.addCallback(object : SurfaceHolder.Callback {
            override fun surfaceCreated(holder: SurfaceHolder) {
                GameBridge.nativeSurfaceCreated(holder.surface)
            }

            override fun surfaceChanged(
                holder: SurfaceHolder,
                format: Int,
                width: Int,
                height: Int,
            ) {
                surfaceWidth = width
                surfaceHeight = height
                cursorX = width / 2f
                cursorY = height / 2f
                // The stub GLFW takes its monitor/window size from here; the
                // game thread applies it on the next event pump.
                CallbackBridge.nativeSendScreenSize(width, height)
            }

            override fun surfaceDestroyed(holder: SurfaceHolder) {
                GameBridge.nativeSurfaceDestroyed()
            }
        })
    }

    @SuppressLint("ClickableViewAccessibility")
    override fun onTouchEvent(event: MotionEvent): Boolean {
        val grabbing = CallbackBridge.isGrabbing()
        if (grabbing != wasGrabbing) {
            // Resync with the game's own recentering on grab transitions; in
            // grab mode only deltas matter, so starting from center is safe.
            wasGrabbing = grabbing
            cursorX = surfaceWidth / 2f
            cursorY = surfaceHeight / 2f
        }

        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                activePointerId = event.getPointerId(0)
                lastTouchX = event.x
                lastTouchY = event.y
                downX = event.x
                downY = event.y
                downTime = event.eventTime
                dragging = false
                if (!grabbing) {
                    moveCursorAbsolute(event.x, event.y)
                }
            }

            MotionEvent.ACTION_MOVE -> {
                val index = event.findPointerIndex(activePointerId)
                if (index < 0) return true
                val x = event.getX(index)
                val y = event.getY(index)
                val dx = x - lastTouchX
                val dy = y - lastTouchY
                lastTouchX = x
                lastTouchY = y

                if (grabbing) {
                    cursorX += dx * LOOK_SENSITIVITY
                    cursorY += dy * LOOK_SENSITIVITY
                    CallbackBridge.nativeSendCursorPos(cursorX, cursorY)
                } else {
                    if (!dragging &&
                        event.eventTime - downTime > tapTimeout &&
                        (kotlin.math.abs(x - downX) > touchSlop ||
                            kotlin.math.abs(y - downY) > touchSlop)
                    ) {
                        // Held and moved: left-button drag (scroll/slider).
                        dragging = true
                        CallbackBridge.nativeSendMouseButton(
                            GLFW_MOUSE_BUTTON_LEFT, GLFW_PRESS, 0,
                        )
                    }
                    moveCursorAbsolute(x, y)
                }
            }

            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                val isTap = event.eventTime - downTime <= tapTimeout &&
                    kotlin.math.abs(event.x - downX) <= touchSlop &&
                    kotlin.math.abs(event.y - downY) <= touchSlop
                if (dragging) {
                    CallbackBridge.nativeSendMouseButton(
                        GLFW_MOUSE_BUTTON_LEFT, GLFW_RELEASE, 0,
                    )
                } else if (isTap && event.actionMasked == MotionEvent.ACTION_UP) {
                    if (!grabbing) {
                        moveCursorAbsolute(event.x, event.y)
                    }
                    CallbackBridge.nativeSendMouseButton(
                        GLFW_MOUSE_BUTTON_LEFT, GLFW_PRESS, 0,
                    )
                    CallbackBridge.nativeSendMouseButton(
                        GLFW_MOUSE_BUTTON_LEFT, GLFW_RELEASE, 0,
                    )
                }
                dragging = false
                activePointerId = MotionEvent.INVALID_POINTER_ID
            }

            MotionEvent.ACTION_POINTER_UP -> {
                // Secondary fingers are ignored; if the tracked finger lifts,
                // end the gesture like a cancel.
                val lifted = event.getPointerId(event.actionIndex)
                if (lifted == activePointerId) {
                    if (dragging) {
                        CallbackBridge.nativeSendMouseButton(
                            GLFW_MOUSE_BUTTON_LEFT, GLFW_RELEASE, 0,
                        )
                        dragging = false
                    }
                    activePointerId = MotionEvent.INVALID_POINTER_ID
                }
            }
        }
        return true
    }

    private fun moveCursorAbsolute(x: Float, y: Float) {
        cursorX = x.coerceIn(0f, surfaceWidth.toFloat())
        cursorY = y.coerceIn(0f, surfaceHeight.toFloat())
        CallbackBridge.nativeSendCursorPos(cursorX, cursorY)
    }
}
