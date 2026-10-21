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
    private lateinit var liveBadge: TextView
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
        val ink = Color.rgb(22, 31, 48)
        val muted = Color.rgb(119, 132, 153)
        val blue = Color.rgb(48, 112, 239)
        val page = Color.rgb(242, 245, 250)
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(22), dp(22), dp(22), dp(28))
            setBackgroundColor(page)
        }

        val brandRow = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        val brandMark = TextView(this).apply {
            text = "V"
            textSize = 17f
            setTextColor(Color.WHITE)
            gravity = Gravity.CENTER
            setTypeface(typeface, Typeface.BOLD)
            background = rounded(blue, blue, dp(14), 0)
        }
        brandRow.addView(brandMark, LinearLayout.LayoutParams(dp(38), dp(38)))
        brandRow.addView(TextView(this).apply {
            text = "VOXLINK"
            textSize = 13f
            letterSpacing = 0.14f
            setTextColor(ink)
            setTypeface(typeface, Typeface.BOLD)
            setPadding(dp(11), 0, 0, 0)
        })
        content.addView(brandRow)

        content.addView(TextView(this).apply {
            text = "Your mic,\nwithout the wires."
            textSize = 31f
            setLineSpacing(dp(1).toFloat(), 1f)
            setTextColor(ink)
            setTypeface(typeface, Typeface.BOLD)
            setPadding(0, dp(25), 0, dp(8))
        })
        content.addView(TextView(this).apply {
            text = "Turn your Android phone into a wireless microphone for your computer."
            textSize = 15f
            setTextColor(muted)
            setLineSpacing(dp(3).toFloat(), 1f)
            setPadding(0, 0, 0, dp(20))
        })

        val liveCard = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(19), dp(18), dp(19), dp(18))
            background = rounded(Color.rgb(20, 30, 49), Color.rgb(20, 30, 49), dp(21), 0)
        }
        val liveHeader = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        liveHeader.addView(TextView(this).apply {
            text = "LIVE AUDIO LINK"
            textSize = 11f
            letterSpacing = 0.11f
            setTextColor(Color.rgb(173, 188, 212))
            setTypeface(typeface, Typeface.BOLD)
        }, LinearLayout.LayoutParams(0, -2, 1f))
        liveBadge = TextView(this).apply {
            text = "●  READY"
            textSize = 10f
            letterSpacing = 0.05f
            setTextColor(Color.rgb(139, 222, 185))
            setPadding(dp(10), dp(7), dp(10), dp(7))
            background = rounded(Color.rgb(34, 61, 65), Color.rgb(34, 61, 65), dp(20), 0)
        }
        liveHeader.addView(liveBadge)
        liveCard.addView(liveHeader)
        liveCard.addView(TextView(this).apply {
            text = "Connect to your computer"
            textSize = 19f
            setTextColor(Color.WHITE)
            setTypeface(typeface, Typeface.BOLD)
            setPadding(0, dp(18), 0, dp(4))
        })
        liveCard.addView(TextView(this).apply {
            text = "48 kHz  ·  Mono  ·  Wi-Fi"
            textSize = 13f
            setTextColor(Color.rgb(173, 188, 212))
        })
        content.addView(liveCard, LinearLayout.LayoutParams(-1, -2).apply { bottomMargin = dp(18) })

        val connectionCard = card()
        connectionCard.addView(TextView(this).apply {
            text = "Connection setup"
            textSize = 18f
            setTextColor(ink)
            setTypeface(typeface, Typeface.BOLD)
        })
        connectionCard.addView(TextView(this).apply {
            text = "Enter the address shown by the Voxlink receiver."
            textSize = 13f
            setTextColor(muted)
            setPadding(0, dp(5), 0, dp(16))
        })
        connectionCard.addView(fieldLabel("COMPUTER IP ADDRESS"))

        addressInput = EditText(this).apply {
            hint = "Computer IP address"
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
            setSingleLine(true)
            textSize = 16f
            setTextColor(ink)
            setHintTextColor(Color.rgb(154, 164, 180))
            setPadding(dp(15), 0, dp(15), 0)
            background = rounded(Color.rgb(247, 249, 252), Color.rgb(229, 233, 240), dp(13), dp(1))
        }
        connectionCard.addView(addressInput, LinearLayout.LayoutParams(-1, dp(54)).apply {
            topMargin = dp(8)
            bottomMargin = dp(16)
        })

        connectionCard.addView(fieldLabel("UDP PORT"))

        portInput = EditText(this).apply {
            setText("4950")
            inputType = InputType.TYPE_CLASS_NUMBER
            setSingleLine(true)
            textSize = 16f
            setTextColor(ink)
            setPadding(dp(15), 0, dp(15), 0)
            background = rounded(Color.rgb(247, 249, 252), Color.rgb(229, 233, 240), dp(13), dp(1))
        }
        connectionCard.addView(portInput, LinearLayout.LayoutParams(-1, dp(54)).apply {
            topMargin = dp(8)
        })
        content.addView(connectionCard)

        val statusCard = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(15), dp(13), dp(15), dp(13))
            background = rounded(Color.WHITE, Color.rgb(231, 235, 242), dp(15), dp(1))
        }
        statusDot = TextView(this).apply {
            text = "●"
            textSize = 16f
            setTextColor(Color.rgb(130, 142, 160))
            setPadding(0, 0, dp(11), 0)
        }
        statusCard.addView(statusDot)
        statusText = TextView(this).apply {
            text = "Ready when you are"
            textSize = 14f
            setTextColor(Color.rgb(69, 81, 101))
            gravity = Gravity.CENTER_VERTICAL
        }
        statusCard.addView(statusText, LinearLayout.LayoutParams(0, -2, 1f))
        val statusParams = LinearLayout.LayoutParams(-1, -2).apply {
            topMargin = dp(14)
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
            background = rounded(blue, blue, dp(15), 0)
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
            background = rounded(Color.rgb(231, 235, 242), Color.rgb(231, 235, 242), dp(15), 0)
            isEnabled = false
            alpha = 0.55f
            setOnClickListener { stopRecording() }
        }
        actions.addView(stopButton, LinearLayout.LayoutParams(dp(100), dp(54)))
        content.addView(actions, LinearLayout.LayoutParams(-1, -2).apply {
            topMargin = dp(14)
        })

        content.addView(TextView(this).apply {
            text = "Start the Voxlink receiver on your computer first. Keep this app open while sending."
            textSize = 13f
            setTextColor(muted)
            gravity = Gravity.CENTER
            setPadding(dp(8), dp(20), dp(8), 0)
        })

        val scroll = ScrollView(this).apply {
            isFillViewport = true
            setBackgroundColor(page)
            addView(content)
        }
        setContentView(scroll)
    }

    private fun card(): LinearLayout = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(dp(18), dp(19), dp(18), dp(18))
        background = rounded(Color.WHITE, Color.rgb(231, 235, 242), dp(19), dp(1))
    }

    private fun fieldLabel(label: String): TextView = TextView(this).apply {
        text = label
        textSize = 11f
        letterSpacing = 0.07f
        setTextColor(Color.rgb(113, 126, 147))
        setTypeface(typeface, Typeface.BOLD)
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
        val active = running && message.startsWith("Sending")
        liveBadge.text = if (active) "●  LIVE" else "●  READY"
        liveBadge.setTextColor(if (active) Color.rgb(142, 236, 187) else Color.rgb(173, 205, 255))
        liveBadge.background = rounded(
            if (active) Color.rgb(31, 77, 64) else Color.rgb(34, 61, 65),
            if (active) Color.rgb(31, 77, 64) else Color.rgb(34, 61, 65),
            dp(20),
            0
        )
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
