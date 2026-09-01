package com.nextos.screenviewer

import java.util.zip.CRC32

/**
 * Wire protocol parser for ScreenViewerOnTablet.
 *
 * This is a 1:1 port of `tools/protocol_py.py` and matches the Rust encoder
 * in `pc-sender/src/enc.rs`. The three implementations are cross-checked via
 * the test_reference_packet value documented in `tools/test_roundtrip.py`
 * and `pc-sender/src/enc.rs::tests::reference_packet_matches_python`.
 *
 * Wire format (little-endian, 24-byte header + payload):
 *   [0..4]   magic         b"NTSS"
 *   [4]      version       (currently 1)
 *   [5]      flags         bit 0 = key frame
 *   [6..8]   width         u16
 *   [8..10]  height        u16
 *   [10..12] format        0 = RGB565, 1 = RGBA32, 2 = JPEG
 *   [12..16] frame_id      u32
 *   [16..20] payload_len   u32
 *   [20..24] crc32         u32 (CRC32 of payload only, IEEE 802.3)
 *   [24..]   payload
 */
object ProtocolParser {
    const val HEADER_LEN: Int = 24
    const val PROTOCOL_VERSION: Int = 1

    /** 4-byte magic: "NTSS" = 0x4E 0x54 0x53 0x53. */
    val MAGIC: ByteArray = byteArrayOf(0x4E, 0x54, 0x53, 0x53)

    // Pixel format codes (match enc::PixelFormat in Rust).
    const val FORMAT_RGB565: Int = 0
    const val FORMAT_RGBA32: Int = 1
    const val FORMAT_JPEG:   Int = 2

    /**
     * Try to decode one wire packet from `packet`.
     *
     * Returns null if any of the following hold:
     *   - the buffer is shorter than the header
     *   - the magic does not match
     *   - the protocol version is unsupported
     *   - the buffer is truncated (header claims more bytes than present)
     *   - the CRC32 of the payload does not match
     */
    fun decode(packet: ByteArray): Frame? {
        if (packet.size < HEADER_LEN) return null
        if (!checkMagic(packet)) return null
        if ((packet[4].toInt() and 0xFF) != PROTOCOL_VERSION) return null

        val flags = packet[5].toInt() and 0xFF
        val width       = readU16LE(packet, 6)
        val height      = readU16LE(packet, 8)
        val format      = readU16LE(packet, 10)
        val frameId     = readU32LE(packet, 12)
        val payloadLen  = readU32LE(packet, 16).toInt() // fits in Int for any sane packet
        val crc         = readU32LE(packet, 20)

        if (packet.size < HEADER_LEN + payloadLen) return null

        val payload = packet.copyOfRange(HEADER_LEN, HEADER_LEN + payloadLen)
        if (crc32(payload) != crc) return null

        return Frame(
            version = packet[4].toInt() and 0xFF,
            isKeyFrame = (flags and 1) != 0,
            width = width,
            height = height,
            format = format,
            frameId = frameId,
            payload = payload,
        )
    }

    /**
     * Read just the payload-length field at offset 16 of a partially-
     * received packet. Returns -1 if the buffer doesn't yet contain a full
     * header. Exposed so the USB reader can know how many more bytes to
     * fetch before it has a complete packet.
     */
    fun peekPayloadLength(buf: ByteArray): Int {
        if (buf.size < HEADER_LEN) return -1
        return readU32LE(buf, 16).toInt()
    }

    private fun checkMagic(packet: ByteArray): Boolean {
        if (packet.size < 4) return false
        for (i in 0..3) {
            if (packet[i] != MAGIC[i]) return false
        }
        return true
    }

    /**
     * Little-endian u16 at the given byte offset. Public so stream readers
     * that pre-parse the header can reuse the same logic.
     */
    fun readU16LE(buf: ByteArray, off: Int): Int =
        (buf[off].toInt() and 0xFF) or
            ((buf[off + 1].toInt() and 0xFF) shl 8)

    /**
     * Little-endian u32 at the given byte offset. Public for the same
     * reason as [readU16LE]. Returns Long because the u32 bit pattern is
     * often combined with u32 fields that could be misinterpreted as signed;
     * using Long forces an explicit `.toInt()` at the call site.
     */
    fun readU32LE(buf: ByteArray, off: Int): Long =
        (buf[off].toInt() and 0xFF).toLong() or
            ((buf[off + 1].toInt() and 0xFF).toLong() shl 8) or
            ((buf[off + 2].toInt() and 0xFF).toLong() shl 16) or
            ((buf[off + 3].toInt() and 0xFF).toLong() shl 24)

    private fun crc32(data: ByteArray): Long {
        val c = CRC32()
        c.update(data)
        return c.value
    }
}
