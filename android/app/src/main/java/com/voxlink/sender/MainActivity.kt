package com.voxlink.sender

import android.Manifest
import android.app.Activity
import android.content.pm.PackageManager
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.os.Bundle
import android.text.InputType
import android.view.Gravity
import android.view.ViewGroup
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.TextView
import java.io.IOException
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress

class MainActivity : Activity() {
    private lateinit var addressInput: EditText
    private lateinit var portInput: EditText
    private lateinit var statusText: TextView
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
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(24, 24, 24, 24)
        }

        root.addView(TextView(this).apply {
            text = "Voxlink audio sender"
            textSize = 22f
        })

        addressInput = EditText(this).apply {
            hint = "PC Wi-Fi IP address"
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
            setSingleLine(true)
        }
        root.addView(addressInput, matchWidth())

        portInput = EditText(this).apply {
            hint = "UDP port"
            setText("4950")
            inputType = InputType.TYPE_CLASS_NUMBER
            setSingleLine(true)
        }
        root.addView(portInput, matchWidth())

        startButton = Button(this).apply {
            text = "Start"
            setOnClickListener { requestStart() }
        }
        root.addView(startButton, matchWidth())

        stopButton = Button(this).apply {
            text = "Stop"
            isEnabled = false
            setOnClickListener { stopRecording() }
        }
        root.addView(stopButton, matchWidth())

        statusText = TextView(this).apply {
            text = "Enter the Linux PC's Wi-Fi IP and start the receiver first."
            textSize = 16f
            gravity = Gravity.START
        }
        root.addView(statusText, matchWidth())
        setContentView(root)
    }

    private fun matchWidth(): LinearLayout.LayoutParams =
        LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)

    private fun requestStart() {
        val address = addressInput.text.toString().trim()
        val port = portInput.text.toString().trim().toIntOrNull()
        if (address.isEmpty() || port == null || port !in 1..65535) {
            statusText.text = "Enter a valid PC IP address and UDP port."
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
            statusText.text = "Microphone permission was denied. Allow it to send audio."
        }
        pendingAddress = null
        pendingPort = null
    }

    private fun startRecording(address: String, port: Int) {
        if (running) return
        running = true
        startButton.isEnabled = false
        stopButton.isEnabled = true
        statusText.text = "Connecting to $address:$port…"

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
                stopButton.isEnabled = false
                statusText.text = failure?.let { "Error: $it" } ?: "Stopped."
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
        runOnUiThread { statusText.text = message }
    }

    private fun stopRecording() {
        if (!running) return
        running = false
        try {
            activeRecorder?.stop()
        } catch (_: IllegalStateException) {
            // Capture may already have stopped after an audio error.
        }
        statusText.text = "Stopping…"
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
