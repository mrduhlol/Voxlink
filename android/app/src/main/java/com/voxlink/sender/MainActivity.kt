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

    @Volatile private var muted = false
    @Volatile private var gain = 1f
    @Volatile private var gateDb = -60
    private var preferredRate = 48000
    private var keepAwake = true
    private lateinit var muteButton: Button
    private lateinit var rateButton: Button
    private lateinit var statsText: TextView
    private lateinit var phoneAddressText: TextView
    private var sessionStarted = 0L
    private var sentPackets = 0L
    private var sentBytes = 0L

    private val pageColor = Color.rgb(16, 19, 17)
    private val panelColor = Color.rgb(23, 27, 24)
    private val lineColor = Color.rgb(42, 49, 44)
    private val limeColor = Color.rgb(194, 240, 106)
    private val brightColor = Color.rgb(238, 240, 235)
    private val mutedColor = Color.rgb(151, 163, 154)
    private lateinit var sessionTitle: TextView
    private lateinit var formatText: TextView
    private lateinit var levelMeter: android.widget.ProgressBar
    private lateinit var levelText: TextView

    private fun label(value: String, size: Float = 13f, color: Int = mutedColor,
                      bold: Boolean = false, mono: Boolean = false): TextView = TextView(this).apply {
        text = value
        textSize = size
        setTextColor(color)
        typeface = if (mono) Typeface.MONOSPACE else Typeface.create("sans-serif", if (bold) Typeface.BOLD else Typeface.NORMAL)
        if (bold) setTypeface(typeface, Typeface.BOLD)
    }

    private fun column(): LinearLayout = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
    private fun row(): LinearLayout = LinearLayout(this).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
    }
    private fun card(): LinearLayout = column().apply {
        setPadding(dp(18), dp(19), dp(18), dp(19))
        background = rounded(panelColor, lineColor, dp(18), dp(1))
    }
    private fun space(parent: LinearLayout, height: Int) {
        parent.addView(android.view.View(this), LinearLayout.LayoutParams(1, dp(height)))
    }
    private fun section(parent: LinearLayout, number: String, title: String) {
        parent.addView(label("$number / $title", 10f, mutedColor, mono = true).apply { letterSpacing = 0.12f })
        space(parent, 14)
    }

    private fun buildControls() {
        window.statusBarColor = pageColor
        window.navigationBarColor = pageColor
        window.decorView.systemUiVisibility = 0
        val content = column().apply {
            setPadding(dp(22), dp(20), dp(22), dp(26))
            setBackgroundColor(pageColor)
        }
        val header = row()
        header.addView(android.widget.ImageView(this).apply {
            setImageResource(com.voxlink.sender.R.drawable.voxlink_icon)
            contentDescription = "Voxlink waveform logo"
        }, LinearLayout.LayoutParams(dp(42), dp(42)))
        val name = column().apply { setPadding(dp(10), 0, 0, 0) }
        name.addView(label("voxlink", 20f, brightColor, bold = true))
        name.addView(label("PHONE AUDIO · LINUX", 8f, mutedColor, mono = true).apply { letterSpacing = 0.12f })
        header.addView(name, LinearLayout.LayoutParams(0, -2, 1f))
        header.addView(label("● LOCAL", 10f, limeColor, mono = true).apply {
            setPadding(dp(10), dp(8), dp(10), dp(8))
            background = rounded(panelColor, lineColor, dp(7), dp(1))
        })
        content.addView(header)
        space(content, 28)
        content.addView(label("WIRELESS AUDIO BRIDGE", 10f, mutedColor, mono = true).apply { letterSpacing = 0.12f })
        space(content, 10)
        content.addView(label("Your phone,", 34f, brightColor, bold = true))
        content.addView(label("a studio microphone.", 30f, limeColor, bold = true))
        space(content, 9)
        content.addView(label("Stream your voice straight to your Linux computer over Wi-Fi.", 14f).apply {
            setLineSpacing(dp(3).toFloat(), 1f)
        })
        space(content, 22)

        val monitor = card()
        val monitorHeader = row()
        monitorHeader.addView(label("01 / AUDIO MONITOR", 10f, mutedColor, mono = true), LinearLayout.LayoutParams(0, -2, 1f))
        liveBadge = label("● STANDBY", 10f, mutedColor, mono = true)
        monitorHeader.addView(liveBadge)
        monitor.addView(monitorHeader)
        space(monitor, 19)
        sessionTitle = label("Ready to go live", 23f, brightColor, bold = true)
        monitor.addView(sessionTitle)
        space(monitor, 6)
        formatText = label("PCM16  /  MONO  /  UDP", 10f, mutedColor, mono = true)
        monitor.addView(formatText)
        space(monitor, 24)
        val meterHeader = row()
        meterHeader.addView(label("MIC INPUT LEVEL", 9f, mutedColor, mono = true), LinearLayout.LayoutParams(0, -2, 1f))
        levelText = label("— dBFS", 10f, mutedColor, mono = true)
        meterHeader.addView(levelText)
        monitor.addView(meterHeader)
        space(monitor, 10)
        levelMeter = android.widget.ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal).apply {
            max = 100
            progress = 0
            progressTintList = android.content.res.ColorStateList.valueOf(limeColor)
            progressBackgroundTintList = android.content.res.ColorStateList.valueOf(lineColor)
            contentDescription = "Microphone input level"
        }
        monitor.addView(levelMeter, LinearLayout.LayoutParams(-1, dp(10)))
        space(monitor, 7)
        monitor.addView(label("−60                 −30                 0 dB", 9f, mutedColor, mono = true))
        space(monitor, 17)
        val statusRow = row()
        statusDot = label("●", 11f, mutedColor).apply { setPadding(0, 0, dp(9), 0) }
        statusRow.addView(statusDot)
        statusText = label("Start the receiver on your computer first.", 12f)
        statusRow.addView(statusText, LinearLayout.LayoutParams(0, -2, 1f))
        monitor.addView(statusRow)
        content.addView(monitor)
        space(content, 14)

        val connection = card()
        section(connection, "02", "CONNECTION")
        connection.addView(label("Where should your audio go?", 18f, brightColor, bold = true))
        space(connection, 6)
        connection.addView(label("Use the PC Wi-Fi address from the desktop app.", 12f))
        space(connection, 18)
        val saved = getPreferences(MODE_PRIVATE)
        connection.addView(label("COMPUTER IP ADDRESS", 10f, mutedColor, mono = true))
        space(connection, 8)
        addressInput = input(saved.getString("host", "") ?: "", "e.g. 192.168.1.7", false)
        addressInput.imeOptions = android.view.inputmethod.EditorInfo.IME_ACTION_NEXT
        connection.addView(addressInput, LinearLayout.LayoutParams(-1, dp(54)))
        space(connection, 16)
        connection.addView(label("UDP PORT", 10f, mutedColor, mono = true))
        space(connection, 8)
        portInput = input(saved.getString("port", "4950") ?: "4950", "4950", true)
        portInput.imeOptions = android.view.inputmethod.EditorInfo.IME_ACTION_DONE
        connection.addView(portInput, LinearLayout.LayoutParams(-1, dp(54)))
        space(connection, 10)
        connection.addView(label("Both devices must be on the same Wi-Fi network.", 11f))
        space(connection, 20)
        val actions = row()
        startButton = Button(this).apply {
            text = "Start streaming  →"
            isAllCaps = false
            textSize = 14f
            setTextColor(pageColor)
            setTypeface(typeface, Typeface.BOLD)
            background = rounded(limeColor, limeColor, dp(12), 0)
            setOnClickListener { requestStart() }
        }
        actions.addView(startButton, LinearLayout.LayoutParams(0, dp(54), 1f).apply { marginEnd = dp(10) })
        stopButton = Button(this).apply {
            text = "Stop"
            isAllCaps = false
            textSize = 14f
            setTextColor(brightColor)
            background = rounded(lineColor, lineColor, dp(12), 0)
            isEnabled = false
            alpha = 0.45f
            setOnClickListener { stopRecording() }
        }
        actions.addView(stopButton, LinearLayout.LayoutParams(dp(80), dp(54)))
        connection.addView(actions)
        content.addView(connection)
        space(content, 14)
        content.addView(featureControls())
        space(content, 19)
        content.addView(label("● Audio stays on your local network.", 11f, mutedColor).apply { gravity = Gravity.CENTER })
        space(content, 7)
        content.addView(label("KEEP VOXLINK OPEN WHILE STREAMING", 8f, mutedColor, mono = true).apply {
            gravity = Gravity.CENTER
            letterSpacing = 0.08f
        })
        setContentView(ScrollView(this).apply {
            isFillViewport = true
            setBackgroundColor(pageColor)
            clipToPadding = false
            addView(content)
            setOnApplyWindowInsetsListener { view, insets ->
                view.setPadding(insets.systemWindowInsetLeft, insets.systemWindowInsetTop, insets.systemWindowInsetRight, insets.systemWindowInsetBottom)
                insets.consumeSystemWindowInsets()
            }
        })
    }

    private fun action(title: String, onClick: () -> Unit): Button = Button(this).apply {
        text = title
        isAllCaps = false
        textSize = 13f
        setTextColor(brightColor)
        background = rounded(lineColor, lineColor, dp(10), 0)
        setOnClickListener { onClick() }
    }

    private fun featureControls(): LinearLayout {
        val prefs = getPreferences(MODE_PRIVATE)
        gain = prefs.getFloat("gain", 1f).coerceIn(0f, 2f)
        gateDb = prefs.getInt("gate", -60).coerceIn(-60, -20)
        preferredRate = prefs.getInt("rate", 48000).let { if (it == 44100) it else 48000 }
        keepAwake = prefs.getBoolean("awake", true)
        val controls = card()
        section(controls, "03", "MICROPHONE CONTROLS")
        muteButton = action("Mute microphone") {
            muted = !muted
            muteButton.text = if (muted) "Unmute microphone" else "Mute microphone"
            muteButton.setTextColor(if (muted) Color.rgb(255, 119, 113) else brightColor)
            if (running) {
                liveBadge.text = if (muted) "● MUTED" else "● SENDING"
                sessionTitle.text = if (muted) "Microphone muted" else "You're on air"
            }
        }
        controls.addView(muteButton, LinearLayout.LayoutParams(-1, dp(48)))
        space(controls, 15)
        val gainLabel = label("MIC GAIN  ·  ${(gain * 100).toInt()}%", 11f, mutedColor, mono = true)
        controls.addView(gainLabel)
        controls.addView(android.widget.SeekBar(this).apply {
            max = 200
            progress = (gain * 100).toInt()
            contentDescription = "Microphone gain from zero to two hundred percent"
            progressTintList = android.content.res.ColorStateList.valueOf(limeColor)
            thumbTintList = android.content.res.ColorStateList.valueOf(limeColor)
            setOnSeekBarChangeListener(object : android.widget.SeekBar.OnSeekBarChangeListener {
                override fun onProgressChanged(bar: android.widget.SeekBar?, value: Int, user: Boolean) {
                    gain = value / 100f
                    gainLabel.text = "MIC GAIN  ·  $value%"
                }
                override fun onStartTrackingTouch(bar: android.widget.SeekBar?) {}
                override fun onStopTrackingTouch(bar: android.widget.SeekBar?) { prefs.edit().putFloat("gain", gain).apply() }
            })
        }, LinearLayout.LayoutParams(-1, dp(48)))
        val gateLabel = label(if (gateDb == -60) "NOISE GATE  ·  OFF" else "NOISE GATE  ·  $gateDb dBFS", 11f, mutedColor, mono = true)
        controls.addView(gateLabel)
        controls.addView(android.widget.SeekBar(this).apply {
            max = 40
            progress = gateDb + 60
            contentDescription = "Noise gate threshold; minimum turns gate off"
            progressTintList = android.content.res.ColorStateList.valueOf(limeColor)
            thumbTintList = android.content.res.ColorStateList.valueOf(limeColor)
            setOnSeekBarChangeListener(object : android.widget.SeekBar.OnSeekBarChangeListener {
                override fun onProgressChanged(bar: android.widget.SeekBar?, value: Int, user: Boolean) {
                    gateDb = value - 60
                    gateLabel.text = if (value == 0) "NOISE GATE  ·  OFF" else "NOISE GATE  ·  $gateDb dBFS"
                }
                override fun onStartTrackingTouch(bar: android.widget.SeekBar?) {}
                override fun onStopTrackingTouch(bar: android.widget.SeekBar?) { prefs.edit().putInt("gate", gateDb).apply() }
            })
        }, LinearLayout.LayoutParams(-1, dp(48)))
        controls.addView(label("Quiet audio below the threshold is silenced. Gain above 100% can clip loud sounds.", 11f))
        space(controls, 14)
        rateButton = action("Sample rate: ${preferredRate / 1000f} kHz") {
            preferredRate = if (preferredRate == 48000) 44100 else 48000
            rateButton.text = "Sample rate: ${preferredRate / 1000f} kHz"
            prefs.edit().putInt("rate", preferredRate).apply()
        }
        controls.addView(rateButton, LinearLayout.LayoutParams(-1, dp(48)))
        space(controls, 10)
        controls.addView(android.widget.Switch(this).apply {
            text = "Keep screen awake while streaming"
            textSize = 13f
            setTextColor(brightColor)
            isChecked = keepAwake
            setOnCheckedChangeListener { _, checked ->
                keepAwake = checked
                prefs.edit().putBoolean("awake", checked).apply()
                updateScreenAwake()
            }
        }, LinearLayout.LayoutParams(-1, dp(52)))
        space(controls, 20)
        section(controls, "04", "CONNECTION TOOLS")
        phoneAddressText = label("", 12f, limeColor, mono = true)
        controls.addView(phoneAddressText)
        fun refreshAddresses() {
            Thread {
                val addresses = try {
                    java.util.Collections.list(java.net.NetworkInterface.getNetworkInterfaces())
                        .filter { it.isUp && !it.isLoopback }
                        .flatMap { iface -> java.util.Collections.list(iface.inetAddresses)
                            .filter { it is java.net.Inet4Address && it.isSiteLocalAddress }
                            .map { "${iface.name}: ${it.hostAddress}" } }
                } catch (_: Exception) { emptyList<String>() }
                runOnUiThread {
                    phoneAddressText.text = if (addresses.isEmpty()) "No local IPv4 address. Connect to Wi-Fi." else addresses.joinToString("\n")
                }
            }.start()
        }
        refreshAddresses()
        space(controls, 8)
        controls.addView(action("Refresh phone addresses") { refreshAddresses() }, LinearLayout.LayoutParams(-1, dp(48)))
        space(controls, 8)
        controls.addView(action("Copy phone addresses") {
            val clipboard = getSystemService(CLIPBOARD_SERVICE) as android.content.ClipboardManager
            clipboard.setPrimaryClip(android.content.ClipData.newPlainText("Phone network addresses", phoneAddressText.text))
            android.widget.Toast.makeText(this, "Addresses copied", android.widget.Toast.LENGTH_SHORT).show()
        }, LinearLayout.LayoutParams(-1, dp(48)))
        space(controls, 8)
        controls.addView(action("Save this computer") {
            val host = addressInput.text.toString().trim()
            val port = portInput.text.toString().toIntOrNull()
            if (host.isBlank() || port == null || port !in 1..65535) {
                setStatus("Enter a valid computer IP and UDP port.", Color.rgb(237, 189, 102))
            } else {
                val profiles = org.json.JSONArray(prefs.getString("profiles", "[]"))
                val entry = "$host:$port"
                val entries = (0 until profiles.length()).map { profiles.getString(it) }.filter { it != entry }
                val updated = org.json.JSONArray()
                updated.put(entry)
                entries.take(9).forEach { updated.put(it) }
                prefs.edit().putString("profiles", updated.toString()).apply()
                android.widget.Toast.makeText(this, "Computer saved", android.widget.Toast.LENGTH_SHORT).show()
            }
        }, LinearLayout.LayoutParams(-1, dp(48)))
        space(controls, 8)
        controls.addView(action("Choose saved computer") {
            if (!running && worker?.isAlive != true) {
                val profiles = org.json.JSONArray(prefs.getString("profiles", "[]"))
                val entries = (0 until profiles.length()).map { profiles.getString(it) }.toTypedArray()
                if (entries.isEmpty()) android.widget.Toast.makeText(this, "Save a computer first", android.widget.Toast.LENGTH_SHORT).show()
                else android.app.AlertDialog.Builder(this).setTitle("Saved computers")
                    .setItems(entries) { _, index ->
                        val entry = entries[index]
                        addressInput.setText(entry.substringBeforeLast(':'))
                        portInput.setText(entry.substringAfterLast(':'))
                    }.setNegativeButton("Cancel", null).show()
            }
        }, LinearLayout.LayoutParams(-1, dp(48)))
        space(controls, 20)
        section(controls, "05", "SESSION STATISTICS")
        statsText = label("00:00  ·  0 packets  ·  0 KB sent", 12f, brightColor, mono = true)
        controls.addView(statsText)
        space(controls, 8)
        controls.addView(label("Counts show audio sent by this phone. Receiver delivery is shown in the desktop app.", 11f))
        space(controls, 14)
        controls.addView(action("Setup & troubleshooting") {
            android.app.AlertDialog.Builder(this).setTitle("Connect Voxlink")
                .setMessage("1. Connect your phone and PC to the same Wi-Fi.\n\n2. In the desktop app, choose the PC's current Wi-Fi address. Use this phone's Wi-Fi address in the Phone IP field. Addresses above are labelled by network interface; avoid VPN addresses.\n\n3. Tap Start listening on the desktop.\n\n4. Enter the PC address here with the same UDP port, then start streaming.\n\nCannot assign requested address means the PC address is wrong. No packets may mean a wrong phone address, firewall, or Wi-Fi client isolation.\n\nKeep this app in the foreground. Muting sends silence and keeps the stream open.")
                .setPositiveButton("Got it", null).show()
        }, LinearLayout.LayoutParams(-1, dp(48)))
        return controls
    }

    private fun updateScreenAwake() {
        if (running && keepAwake) window.addFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        else window.clearFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    }

    private fun input(value: String, placeholder: String, numeric: Boolean): EditText = EditText(this).apply {
        setText(value)
        hint = placeholder
        inputType = if (numeric) InputType.TYPE_CLASS_NUMBER else InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
        setSingleLine(true)
        textSize = 15f
        typeface = Typeface.MONOSPACE
        setTextColor(brightColor)
        setHintTextColor(mutedColor)
        setPadding(dp(14), 0, dp(14), 0)
        background = rounded(pageColor, lineColor, dp(10), dp(1))
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
        if (running || worker?.isAlive == true) return
        getPreferences(MODE_PRIVATE).edit().putString("host", address).putString("port", port.toString()).apply()
        addressInput.isEnabled = false
        portInput.isEnabled = false
        running = true
        rateButton.isEnabled = false
        sessionStarted = android.os.SystemClock.elapsedRealtime()
        sentPackets = 0
        sentBytes = 0
        statsText.text = "00:00  ·  0 packets  ·  0 KB sent"
        updateScreenAwake()
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
            runOnUiThread { formatText.text = "${sampleRate / 1000f} kHz  /  MONO  /  PCM16" }
            var lastMeterUpdate = 0L
            var sequence = 0
            while (running) {
                val count = recorder.read(samples, 0, samples.size, AudioRecord.READ_BLOCKING)
                if (count < 0) {
                    if (!running) break
                    throw IOException("Audio capture failed (code $count)")
                }
                if (count == 0) continue

                var inputPeak = 0
                for (index in 0 until count) inputPeak = maxOf(inputPeak, kotlin.math.abs(samples[index].toInt()))
                val inputDb = if (inputPeak == 0) -96.0 else 20 * kotlin.math.log10(inputPeak / 32768.0)
                val silence = muted || (gateDb > -60 && inputDb < gateDb)
                val packetGain = gain
                for (index in 0 until count) {
                    samples[index] = if (silence) 0 else (samples[index] * packetGain).toInt().coerceIn(-32768, 32767).toShort()
                }
                val now = android.os.SystemClock.elapsedRealtime()
                if (now - lastMeterUpdate >= 80) {
                    lastMeterUpdate = now
                    var peak = 0
                    for (index in 0 until count) peak = maxOf(peak, kotlin.math.abs(samples[index].toInt()))
                    val db = if (peak == 0) -60.0 else maxOf(-60.0, 20 * kotlin.math.log10(peak / 32768.0))
                    runOnUiThread {
                        if (running) {
                            levelMeter.progress = ((db + 60) / 60 * 100).toInt()
                            levelText.text = if (muted) "MUTED" else "${db.toInt()} dBFS"
                            val elapsed = (android.os.SystemClock.elapsedRealtime() - sessionStarted) / 1000
                            statsText.text = "%02d:%02d  ·  %d packets  ·  %d KB sent".format(elapsed / 60, elapsed % 60, sentPackets, sentBytes / 1024)
                            liveBadge.text = if (muted) "● MUTED" else "● SENDING"
                            sessionTitle.text = if (muted) "Microphone muted" else "You're on air"
                        }
                    }
                }
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
                sentPackets++
                sentBytes += packetBytes.size
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
                updateScreenAwake()
                rateButton.isEnabled = true
                addressInput.isEnabled = true
                portInput.isEnabled = true
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
        val rates = if (preferredRate == 44100) intArrayOf(44100, 48000) else intArrayOf(48000, 44100)
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
        val error = message.startsWith("Error") || message.contains("denied") || message.startsWith("Enter")
        liveBadge.text = when { active -> "● SENDING"; error -> "● ERROR"; running -> "● STARTING"; else -> "● STANDBY" }
        liveBadge.setTextColor(if (error) Color.rgb(255, 119, 113) else if (running) limeColor else mutedColor)
        sessionTitle.text = when { active -> "You're on air"; error -> "Check your connection"; running -> "Opening microphone…"; else -> "Ready to go live" }
        if (!running) {
            levelMeter.progress = 0
            levelText.text = "— dBFS"
        }
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

    override fun onStop() {
        // Recording is foreground-only; avoid capturing after the app is hidden.
        stopRecording()
        super.onStop()
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
