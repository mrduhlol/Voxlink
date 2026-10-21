package com.voxlink.sender

import android.Manifest
import android.app.Activity
import android.content.pm.PackageManager
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.os.Bundle
import android.text.InputType
import android.view.Gravity
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import java.io.IOException
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress

class MainActivity : Activity() {
    private lateinit var addressInput: EditText
    private lateinit var portInput: EditText
    private lateinit var statusText: TextView
    private lateinit var statusDot: TextView
    private lateinit var startButton: Button
    private lateinit var stopButton: Button

    @Volatile private var running = false
    @Volatile private var activeRecorder: AudioRecord? = null
    private var worker: Thread? = null
    private var pendingAddress: String? = null
    private var pendingPort: Int? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        buildControls()
    }

    private fun buildControls() {
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(24), dp(28), dp(24), dp(28))
            setBackgroundColor(Color.rgb(245, 247, 251))
        }

        content.addView(TextView(this).apply {
            text = "VOXLINK"
            textSize = 13f
            letterSpacing = 0.16f
            setTextColor(Color.rgb(33, 103, 220))
            setTypeface(typeface, Typeface.BOLD)
        })
        content.addView(TextView(this).apply {
            text = "Wireless microphone"
            textSize = 28f
            setTextColor(Color.rgb(24, 32, 48))
            setTypeface(typeface, Typeface.BOLD)
            setPadding(0, dp(8), 0, dp(4))
        })
        content.addView(TextView(this).apply {
            text = "Send your phone's mic audio to your Linux computer over Wi-Fi."
            textSize = 15f
            setTextColor(Color.rgb(99, 111, 130))
            setPadding(0, 0, 0, dp(24))
        })

        val connectionCard = card()
        connectionCard.addView(TextView(this).apply {
            text = "COMPUTER"
            textSize = 12f
            letterSpacing = 0.08f
            setTextColor(Color.rgb(105, 117, 137))
            setTypeface(typeface, Typeface.BOLD)
        })

        addressInput = EditText(this).apply {
            hint = "Computer IP address"
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
            setSingleLine(true)
            textSize = 16f
            setTextColor(Color.rgb(24, 32, 48))
            setHintTextColor(Color.rgb(145, 154, 170))
            setPadding(dp(14), 0, dp(14), 0)
            background = rounded(Color.WHITE, Color.rgb(220, 226, 235), dp(12), dp(1))
        }
        connectionCard.addView(addressInput, LinearLayout.LayoutParams(-1, dp(54)).apply {
            topMargin = dp(8)
            bottomMargin = dp(18)
        })

        connectionCard.addView(TextView(this).apply {
            text = "UDP PORT"
            textSize = 12f
            letterSpacing = 0.08f
            setTextColor(Color.rgb(105, 117, 137))
            setTypeface(typeface, Typeface.BOLD)
        })

        portInput = EditText(this).apply {
            setText("4950")
            inputType = InputType.TYPE_CLASS_NUMBER
            setSingleLine(true)
            textSize = 16f
            setTextColor(Color.rgb(24, 32, 48))
            setPadding(dp(14), 0, dp(14), 0)
            background = rounded(Color.WHITE, Color.rgb(220, 226, 235), dp(12), dp(1))
        }
        connectionCard.addView(portInput, LinearLayout.LayoutParams(-1, dp(54)).apply {
            topMargin = dp(8)
        })
        content.addView(connectionCard)

        val statusCard = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(16), dp(15), dp(16), dp(15))
            background = rounded(Color.WHITE, Color.rgb(231, 235, 242), dp(16), dp(1))
        }
        statusDot = TextView(this).apply {
            text = "●"
            textSize = 16f
            setTextColor(Color.rgb(130, 142, 160))
            setPadding(0, 0, dp(12), 0)
        }
        statusCard.addView(statusDot)
        statusText = TextView(this).apply {
            text = "Ready to connect"
            textSize = 14f
            setTextColor(Color.rgb(69, 81, 101))
            gravity = Gravity.CENTER_VERTICAL
        }
        statusCard.addView(statusText, LinearLayout.LayoutParams(0, -2, 1f))
        val statusParams = LinearLayout.LayoutParams(-1, -2).apply {
            topMargin = dp(16)
        }
        content.addView(statusCard, statusParams)

        val actions = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        startButton = Button(this).apply {
            text = "Start sending"
            isAllCaps = false
            textSize = 15f
            setTextColor(Color.WHITE)
            setTypeface(typeface, Typeface.BOLD)
            background = rounded(Color.rgb(38, 105, 224), Color.rgb(38, 105, 224), dp(14), 0)
            setOnClickListener { requestStart() }
        }
        actions.addView(startButton, LinearLayout.LayoutParams(0, dp(54), 1f).apply {
            marginEnd = dp(8)
        })
        stopButton = Button(this).apply {
            text = "Stop"
            isAllCaps = false
            textSize = 15f
            setTextColor(Color.rgb(65, 78, 99))
            background = rounded(Color.rgb(231, 235, 242), Color.rgb(231, 235, 242), dp(14), 0)
            isEnabled = false
            alpha = 0.55f
            setOnClickListener { stopRecording() }
        }
        actions.addView(stopButton, LinearLayout.LayoutParams(dp(100), dp(54)))
        content.addView(actions, LinearLayout.LayoutParams(-1, -2).apply {
            topMargin = dp(14)
        })

        content.addView(TextView(this).apply {
            text = "Keep Voxlink open while sending. Start the receiver on your computer first."
            textSize = 13f
            setTextColor(Color.rgb(119, 130, 148))
            gravity = Gravity.CENTER
            setPadding(dp(8), dp(20), dp(8), 0)
        })

        val scroll = ScrollView(this).apply {
            isFillViewport = true
            setBackgroundColor(Color.rgb(245, 247, 251))
            addView(content)
        }
        setContentView(scroll)
    }

    private fun card(): LinearLayout = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(dp(18), dp(18), dp(18), dp(18))
        background = rounded(Color.WHITE, Color.rgb(231, 235, 242), dp(18), dp(1))
    }

    private fun rounded(fill: Int, stroke: Int, radius: Int, strokeWidth: Int): GradientDrawable =
        GradientDrawable().apply {
            setColor(fill)
            cornerRadius = radius.toFloat()
            if (strokeWidth > 0) setStroke(strokeWidth, stroke)
        }

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

    private fun requestStart() {
        val address = addressInput.text.toString().trim()
        val port = portInput.text.toString().trim().toIntOrNull()
        if (address.isEmpty() || port == null || port !in 1..65535) {
            setStatus("Enter a valid computer IP and UDP port.", Color.rgb(197, 116, 20))
            return
        }

        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            pendingAddress = address
            pendingPort = port
            requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), REQUEST_MICROPHONE)
            return
        }
        startRecording(address, port)
    }

    @Deprecated("Uses the platform permission callback for a dependency-free prototype")
    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode != REQUEST_MICROPHONE) return

        if (grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED) {
            val address = pendingAddress
            val port = pendingPort
            if (address != null && port != null) startRecording(address, port)
        } else {
            setStatus("Microphone permission was denied.", Color.rgb(197, 72, 72))
        }
        pendingAddress = null
        pendingPort = null
    }

    private fun startRecording(address: String, port: Int) {
        if (running) return
        running = true
        startButton.isEnabled = false
        startButton.alpha = 0.55f
        stopButton.isEnabled = true
        stopButton.alpha = 1f
        setStatus("Connecting to $address:$port…", Color.rgb(38, 105, 224))

        worker = Thread({ streamAudio(address, port) }, "voxlink-audio-sender")
        worker?.start()
    }

    private fun streamAudio(host: String, port: Int) {
        var recorder: AudioRecord? = null
        var socket: DatagramSocket? = null
        var failure: String? = null

        try {
            val destination = InetAddress.getByName(host)
            socket = DatagramSocket()
            recorder = createRecorder()
            activeRecorder = recorder
            recorder.startRecording()
            if (recorder.recordingState != AudioRecord.RECORDSTATE_RECORDING) {
                throw IOException("Android did not start microphone capture")
            }
            showStatus("Sending ${recorder.sampleRate} Hz mono audio to $host:$port")

            val sampleRate = recorder.sampleRate
            val samples = ShortArray(PACKET_SAMPLES)
            var sequence = 0
            while (running) {
                val count = recorder.read(samples, 0, samples.size, AudioRecord.READ_BLOCKING)
                if (count < 0) {
                    if (!running) break
                    throw IOException("Audio capture failed (code $count)")
                }
                if (count == 0) continue

                val packetBytes = ByteArray(HEADER_SIZE + count * 2)
                packetBytes[0] = 'V'.code.toByte()
                packetBytes[1] = 'X'.code.toByte()
                packetBytes[2] = 'L'.code.toByte()
                packetBytes[3] = '1'.code.toByte()
                writeIntBigEndian(packetBytes, 4, sequence)
                writeIntBigEndian(packetBytes, 8, sampleRate)
                for (index in 0 until count) {
                    val sample = samples[index].toInt()
                    packetBytes[HEADER_SIZE + index * 2] = (sample and 0xff).toByte()
                    packetBytes[HEADER_SIZE + index * 2 + 1] = (sample shr 8).toByte()
                }
                socket.send(DatagramPacket(packetBytes, packetBytes.size, destination, port))
                sequence++
            }
        } catch (error: Exception) {
            if (running) failure = error.message ?: error.javaClass.simpleName
        } finally {
            running = false
            activeRecorder = null
            try {
                if (recorder?.recordingState == AudioRecord.RECORDSTATE_RECORDING) recorder.stop()
            } catch (_: IllegalStateException) {
                // The Stop button may already have stopped the recorder.
            }
            recorder?.release()
            socket?.close()
            runOnUiThread {
                startButton.isEnabled = true
                startButton.alpha = 1f
                stopButton.isEnabled = false
                stopButton.alpha = 0.55f
                if (failure != null) setStatus("Error: $failure", Color.rgb(197, 72, 72))
                else setStatus("Stopped", Color.rgb(119, 130, 148))
            }
        }
    }

    private fun createRecorder(): AudioRecord {
        val rates = intArrayOf(48_000, 44_100)
        for (rate in rates) {
            val minimum = AudioRecord.getMinBufferSize(
                rate,
                AudioFormat.CHANNEL_IN_MONO,
                AudioFormat.ENCODING_PCM_16BIT
            )
            if (minimum <= 0) continue

            val candidate = try {
                AudioRecord(
                    MediaRecorder.AudioSource.MIC,
                    rate,
                    AudioFormat.CHANNEL_IN_MONO,
                    AudioFormat.ENCODING_PCM_16BIT,
                    maxOf(minimum * 2, PACKET_SAMPLES * 4)
                )
            } catch (_: IllegalArgumentException) {
                continue
            }
            if (candidate.state == AudioRecord.STATE_INITIALIZED) return candidate
            candidate.release()
        }
        throw IOException("Could not initialize 48 kHz or 44.1 kHz microphone capture")
    }

    private fun writeIntBigEndian(bytes: ByteArray, offset: Int, value: Int) {
        bytes[offset] = (value ushr 24).toByte()
        bytes[offset + 1] = (value ushr 16).toByte()
        bytes[offset + 2] = (value ushr 8).toByte()
        bytes[offset + 3] = value.toByte()
    }

    private fun showStatus(message: String) {
        runOnUiThread { setStatus(message, Color.rgb(38, 153, 105)) }
    }

    private fun setStatus(message: String, color: Int) {
        statusText.text = message
        statusDot.setTextColor(color)
    }

    private fun stopRecording() {
        if (!running) return
        running = false
        try {
            activeRecorder?.stop()
        } catch (_: IllegalStateException) {
            // Capture may already have stopped after an audio error.
        }
        setStatus("Stopping…", Color.rgb(197, 116, 20))
    }

    override fun onDestroy() {
        stopRecording()
        super.onDestroy()
    }

    companion object {
        private const val REQUEST_MICROPHONE = 1001
        private const val PACKET_SAMPLES = 480
        private const val HEADER_SIZE = 12
    }
}
