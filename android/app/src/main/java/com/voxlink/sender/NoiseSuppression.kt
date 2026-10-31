package com.voxlink.sender

import android.media.audiofx.AudioEffect
import android.media.audiofx.NoiseSuppressor

/** Device-provided noise suppression attached only to this microphone capture session. */
object NoiseSuppression {
    fun isSupported(): Boolean = try { NoiseSuppressor.isAvailable() } catch (_: RuntimeException) { false }

    class Session(private val effect: NoiseSuppressor?, val description: String) : AutoCloseable {
        override fun close() {
            try { effect?.release() } catch (_: RuntimeException) { }
        }
    }

    fun open(audioSession: Int, enabled: Boolean): Session {
        if (!isSupported()) return Session(null, "Noise suppression unavailable on this phone")
        var effect: NoiseSuppressor? = null
        return try {
            effect = NoiseSuppressor.create(audioSession)
            if (effect == null || effect.setEnabled(enabled) != AudioEffect.SUCCESS || effect.enabled != enabled) {
                effect?.release()
                Session(null, "Noise suppression unavailable for this recording")
            } else Session(effect, if (enabled) "Noise suppression on" else "Noise suppression off")
        } catch (_: RuntimeException) {
            try { effect?.release() } catch (_: RuntimeException) { }
            Session(null, "Noise suppression unavailable for this recording")
        }
    }
}
