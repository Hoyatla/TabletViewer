package com.nextos.screenviewer

/**
 * One decoded frame from the wire protocol.
 *
 * @property version          protocol version (currently always 1)
 * @property isKeyFrame       true if this is a key frame (no delta support yet)
 * @property width            pixel width
 * @property height           pixel height
 * @property format           0 = RGB565, 1 = RGBA32, 2 = JPEG
 * @property frameId          monotonically increasing per session
 * @property payload          raw pixel bytes (no header, no CRC)
 */
data class Frame(
    val version: Int,
    val isKeyFrame: Boolean,
    val width: Int,
    val height: Int,
    val format: Int,
    val frameId: Long,
    val payload: ByteArray,
) {
    // ByteArray breaks data-class equals/hashCode by default — override
    // so we don't accidentally compare by reference.
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is Frame) return false
        return version == other.version &&
            isKeyFrame == other.isKeyFrame &&
            width == other.width &&
            height == other.height &&
            format == other.format &&
            frameId == other.frameId &&
            payload.contentEquals(other.payload)
    }

    override fun hashCode(): Int {
        var result = version
        result = 31 * result + isKeyFrame.hashCode()
        result = 31 * result + width
        result = 31 * result + height
        result = 31 * result + format
        result = 31 * result + frameId.hashCode()
        result = 31 * result + payload.contentHashCode()
        return result
    }
}
