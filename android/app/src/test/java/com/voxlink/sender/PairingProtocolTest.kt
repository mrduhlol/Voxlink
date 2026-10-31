package com.voxlink.sender

import org.junit.Assert.*
import org.junit.Test
import java.net.DatagramSocket
import java.net.DatagramPacket
import java.net.InetAddress
import kotlin.concurrent.thread

class PairingProtocolTest {
    private fun reply() = "VXLR482913".toByteArray() + byteArrayOf(0x13, 0x56)
    @Test fun packetFormats() {
        assertArrayEquals("VXLD".toByteArray(), PairingProtocol.buildProbe())
        assertArrayEquals("VXLC482913".toByteArray(), PairingProtocol.buildClaim("482913"))
        assertThrows(IllegalArgumentException::class.java) { PairingProtocol.buildClaim("4829") }
        assertEquals(PairingProtocol.Reply("192.168.1.5",4950,"482913"), PairingProtocol.parseReply(reply(),12,"192.168.1.5"))
        assertNull(PairingProtocol.parseReply(reply(),8,"192.168.1.5"))
        assertNull(PairingProtocol.parseReply("NOPE482913xx".toByteArray(),12,"192.168.1.5"))
        assertNull(PairingProtocol.parseReply("VXLR48291a".toByteArray() + byteArrayOf(0x13,0x56),12,"192.168.1.5"))
        assertTrue(PairingProtocol.isAck("VXLA".toByteArray(),4))
        assertFalse(PairingProtocol.isAck("VXLD".toByteArray(),4))
        assertFalse(PairingProtocol.isAck("VXLA".toByteArray(),3))
        assertFalse(PairingProtocol.isAck("VXLAjunk".toByteArray(),8))
        assertTrue(PairingProtocol.isPending("VXLP".toByteArray(),4))
        assertTrue(PairingProtocol.isRejected("VXLN".toByteArray(),4))
        assertFalse(PairingProtocol.isPending("VXLA".toByteArray(),4))
        assertFalse(PairingProtocol.isRejected("VXLP".toByteArray(),4))
        assertFalse(PairingProtocol.isPending("VXLP".toByteArray(),3))
        assertFalse(PairingProtocol.isRejected("VXLNjunk".toByteArray(),8))
    }
    @Test fun uriMatchesDesktop() {
        assertEquals(PairingProtocol.PairingTarget("192.168.1.5",4950,"482913"),PairingProtocol.parsePairingUri("voxlink://192.168.1.5:4950?code=482913"))
        for (uri in listOf("https://192.168.1.5:4950?code=482913","voxlink://192.168.1.5","voxlink://192.168.1.5:99999?code=482913","voxlink://localhost:4950?code=482913","voxlink://192.168.1.5:4951?code=482913","voxlink://192.168.1.5:4950?code=48291a")) assertNull(uri,PairingProtocol.parsePairingUri(uri))
    }
    @Test fun destinationRejectsLocalhostAndLinkLocal() {
        for (host in listOf("localhost","127.0.0.1","127.1.2.3","0.0.0.0","169.254.10.10")) assertEquals(PairingProtocol.LOCAL_ADDRESS_ERROR,PairingProtocol.validateDestination(host))
        assertNull(PairingProtocol.validateDestination("192.168.1.5"))
        assertNull(PairingProtocol.validateDestination("10.0.2.2"))
        assertNotNull(PairingProtocol.validateDestination("999.2.3.4"))
        assertEquals("192.168.1.255",PairingProtocol.subnetBroadcastOf(InetAddress.getByName("192.168.1.42")))
        assertEquals("10.0.2.255",PairingProtocol.subnetBroadcastOf(InetAddress.getByName("10.0.2.15")))
    }
    @Test fun claimAcknowledgementAndTimeout() {
        DatagramSocket(0,InetAddress.getLoopbackAddress()).use { server ->
            server.soTimeout = 2000
            val worker = thread {
                val packet = DatagramPacket(ByteArray(32),32); server.receive(packet)
                assertEquals("VXLC482913",String(packet.data,0,packet.length))
                val ack = "VXLA".toByteArray(); server.send(DatagramPacket(ack,ack.size,packet.address,packet.port))
            }
            assertTrue(PairingProtocol.claimWithAck("127.0.0.1",server.localPort,"482913"))
            worker.join()
            val start = System.nanoTime()
            assertFalse(PairingProtocol.claimWithAck("127.0.0.1",server.localPort,"482913",400))
            assertTrue((System.nanoTime()-start)/1_000_000 < 1500)
        }
    }
    @Test fun discoveryCollectsAndDeduplicatesReplies() {
        DatagramSocket(0,InetAddress.getLoopbackAddress()).use { server ->
            server.soTimeout = 1500
            val worker = thread {
                repeat(3) {
                    val packet = DatagramPacket(ByteArray(32),32); server.receive(packet)
                    assertEquals("VXLD",String(packet.data,0,packet.length))
                    val response = reply(); server.send(DatagramPacket(response,response.size,packet.address,packet.port))
                }
            }
            assertEquals(listOf(PairingProtocol.Reply("127.0.0.1",4950,"482913")),PairingProtocol.discover(listOf("127.0.0.1"),server.localPort,650))
            worker.join()
        }
    }
    private val pendingBytes = "VXLP".toByteArray()
    private val approvedBytes = "VXLA".toByteArray()
    private val rejectedBytes = "VXLN".toByteArray()

    private fun serve(server: DatagramSocket, vararg decisions: ByteArray): Pair<Int, Thread> = server.localPort to thread {
        for (decision in decisions) {
            val packet = DatagramPacket(ByteArray(32),32); server.receive(packet)
            assertEquals("VXLC482913",String(packet.data,0,packet.length))
            server.send(DatagramPacket(decision,decision.size,packet.address,packet.port))
        }
    }

    @Test fun requestPairingWaitsForPendingThenApprovalOnThePc() {
        DatagramSocket(0,InetAddress.getLoopbackAddress()).use { server ->
            val pendingCount = java.util.concurrent.atomic.AtomicInteger()
            val pendings = java.util.Collections.synchronizedList(mutableListOf<Long>())
            val (port, worker) = serve(server, pendingBytes, approvedBytes)
            val start = System.nanoTime()
            assertEquals(PairingProtocol.PairingDecision.APPROVED,
                PairingProtocol.requestPairing("127.0.0.1",port,"482913",5_000,
                    onPending = { pendings.add(pendingCount.incrementAndGet().toLong()) }))
            assertTrue((System.nanoTime()-start)/1_000_000 < 4_000)
            assertTrue("pending callback should fire before approval",pendings.isNotEmpty())
            worker.join()
        }
    }
    @Test fun requestPairingReportsRejectionWithoutSendingAudio() {
        DatagramSocket(0,InetAddress.getLoopbackAddress()).use { server ->
            val (port, worker) = serve(server, rejectedBytes)
            val start = System.nanoTime()
            assertEquals(PairingProtocol.PairingDecision.REJECTED,
                PairingProtocol.requestPairing("127.0.0.1",port,"482913",5_000))
            // Rejection short-circuits; it must not burn the whole wait window.
            assertTrue((System.nanoTime()-start)/1_000_000 < 3_000)
            worker.join()
        }
    }
    @Test fun legacyImmediateAckStillApproves() {
        DatagramSocket(0,InetAddress.getLoopbackAddress()).use { server ->
            val (port, worker) = serve(server, approvedBytes, approvedBytes)
            val start = System.nanoTime()
            assertEquals(PairingProtocol.PairingDecision.APPROVED,
                PairingProtocol.requestPairing("127.0.0.1",port,"482913",5_000))
            assertTrue(PairingProtocol.claimWithAck("127.0.0.1",port,"482913"))
            assertTrue((System.nanoTime()-start)/1_000_000 < 3_000)
            worker.join()
        }
    }
    @Test fun pendingCallbackFiresOncePerTransitionNotPerRetry() {
        DatagramSocket(0,InetAddress.getLoopbackAddress()).use { server ->
            // PC answers VXLP to every claim and never decides — hits the timeout.
            val vxlpReplies = java.util.concurrent.atomic.AtomicInteger()
            val worker = thread {
                while (!server.isClosed) {
                    try {
                        server.soTimeout = 50
                        val packet = DatagramPacket(ByteArray(32),32); server.receive(packet)
                        assertEquals("VXLC482913",String(packet.data,0,packet.length))
                        server.send(DatagramPacket(pendingBytes,pendingBytes.size,packet.address,packet.port))
                        vxlpReplies.incrementAndGet()
                    } catch (_: java.net.SocketTimeoutException) { }
                      catch (_: java.net.SocketException) { break }
                }
            }
            val pendings = java.util.Collections.synchronizedList(mutableListOf<Boolean>())
            val start = System.nanoTime()
            assertEquals(PairingProtocol.PairingDecision.TIMEDOUT,
                PairingProtocol.requestPairing("127.0.0.1",server.localPort,"482913",1_000,
                    onPending = { pendings.add(true) }))
            assertTrue((System.nanoTime()-start)/1_000_000 < 2_000)
            worker.interrupt(); worker.join(1000)
            assertTrue("test setup must receive several VXLP replies across retries",vxlpReplies.get() >= 2)
            assertEquals("onPending must fire on the pending transition only, not every retry",1,pendings.size)
        }
    }
    @Test fun pendingUntilTimeoutAndCancelStops() {
        DatagramSocket(0,InetAddress.getLoopbackAddress()).use { server ->
            // PC answers VXLP to every claim and never decides.
            val worker = thread {
                while (!Thread.currentThread().isInterrupted && !server.isClosed) {
                    try {
                        val packet = DatagramPacket(ByteArray(32),32); server.receive(packet)
                        server.send(DatagramPacket(pendingBytes,pendingBytes.size,packet.address,packet.port))
                    } catch (_: java.net.SocketTimeoutException) { }
                      catch (_: java.net.SocketException) { break }
                }
            }
            val pendings = java.util.Collections.synchronizedList(mutableListOf<Boolean>())
            var start = System.nanoTime()
            assertEquals(PairingProtocol.PairingDecision.TIMEDOUT,
                PairingProtocol.requestPairing("127.0.0.1",server.localPort,"482913",650,
                    onPending = { pendings.add(true) }))
            assertTrue((System.nanoTime()-start)/1_000_000 < 2_000)
            assertTrue("pending callback should fire on VXLP",pendings.isNotEmpty())
            // Stop/onStop cancellation: shouldContinue turns false right after the first claim.
            val claims = java.util.concurrent.atomic.AtomicInteger()
            start = System.nanoTime()
            assertEquals(PairingProtocol.PairingDecision.CANCELLED,
                PairingProtocol.requestPairing("127.0.0.1",server.localPort,"482913",5_000,
                    shouldContinue = { claims.incrementAndGet() < 2 }))
            assertTrue("cancel must be prompt",(System.nanoTime()-start)/1_000_000 < 1_500)
            assertTrue("cancel should not claim repeatedly",claims.get() <= 2)
            worker.interrupt(); worker.join(1000)
        }
    }
}
