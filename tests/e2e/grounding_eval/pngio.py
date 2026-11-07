"""PNG in and out with the standard library only, for the grounding evaluation's grid images: the runner's image has
Python but no imaging library. Reads 8-bit RGB or RGBA, non-interlaced (what the capture writes), undoing each row's
filter; writes 8-bit RGB with no filter."""

import struct
import zlib

SIGNATURE = b"\x89PNG\r\n\x1a\n"


class Image:
    """An RGB image: width, height and its pixels, three bytes each, row by row."""

    def __init__(self, width, height, rgb):
        assert len(rgb) == width * height * 3, (width, height, len(rgb))
        self.width, self.height, self.rgb = width, height, rgb

    def pixel(self, x, y):
        at = (y * self.width + x) * 3
        return tuple(self.rgb[at:at + 3])


def chunks(data):
    at = len(SIGNATURE)
    while at < len(data):
        (length,) = struct.unpack(">I", data[at:at + 4])
        kind = data[at + 4:at + 8]
        yield kind, data[at + 8:at + 8 + length]
        at += 12 + length


def paeth(a, b, c):
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    return a if pa <= pb and pa <= pc else b if pb <= pc else c


def decode(data):
    """An Image from PNG bytes (8-bit RGB or RGBA, non-interlaced; the alpha is dropped)."""
    if data[:8] != SIGNATURE:
        raise ValueError("not a PNG")
    header, idat = None, bytearray()
    for kind, body in chunks(data):
        if kind == b"IHDR":
            header = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            idat += body
    width, height, depth, colour, _compression, _filter, interlace = header
    if depth != 8 or colour not in (2, 6) or interlace:
        raise ValueError(f"only 8-bit RGB or RGBA, non-interlaced (depth {depth}, colour {colour}, "
                         f"interlace {interlace})")
    channels = 3 if colour == 2 else 4
    stride = width * channels
    raw = zlib.decompress(bytes(idat))
    rows, previous = [], bytearray(stride)
    for y in range(height):
        at = y * (stride + 1)
        kind, line = raw[at], bytearray(raw[at + 1:at + 1 + stride])
        if kind == 1:
            for i in range(channels, stride):
                line[i] = (line[i] + line[i - channels]) & 255
        elif kind == 2:
            for i in range(stride):
                line[i] = (line[i] + previous[i]) & 255
        elif kind == 3:
            for i in range(stride):
                left = line[i - channels] if i >= channels else 0
                line[i] = (line[i] + ((left + previous[i]) >> 1)) & 255
        elif kind == 4:
            for i in range(stride):
                left = line[i - channels] if i >= channels else 0
                upper_left = previous[i - channels] if i >= channels else 0
                line[i] = (line[i] + paeth(left, previous[i], upper_left)) & 255
        elif kind != 0:
            raise ValueError(f"row {y}: unknown filter {kind}")
        rows.append(line)
        previous = line
    rgb = bytearray()
    for line in rows:
        if channels == 3:
            rgb += line
        else:
            for i in range(0, stride, 4):
                rgb += line[i:i + 3]
    return Image(width, height, rgb)


def chunk(kind, body):
    return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)


def encode(image, filters=None):
    """PNG bytes of an RGB Image; each row unfiltered unless `filters` gives one filter type per row (tests)."""
    stride = image.width * 3
    raw = bytearray()
    previous = bytearray(stride)
    for y in range(image.height):
        line = image.rgb[y * stride:(y + 1) * stride]
        kind = filters[y] if filters else 0
        raw.append(kind)
        raw += filtered(kind, line, previous)
        previous = line
    header = struct.pack(">IIBBBBB", image.width, image.height, 8, 2, 0, 0, 0)
    return SIGNATURE + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(bytes(raw), 6)) + chunk(b"IEND", b"")


def filtered(kind, line, previous):
    if kind == 0:
        return bytes(line)
    out = bytearray(len(line))
    for i in range(len(line)):
        left = line[i - 3] if i >= 3 else 0
        upper_left = previous[i - 3] if i >= 3 else 0
        predictor = {1: left, 2: previous[i], 3: (left + previous[i]) >> 1,
                     4: paeth(left, previous[i], upper_left)}[kind]
        out[i] = (line[i] - predictor) & 255
    return bytes(out)


def encode_filtered(image):
    """The image encoded once with each of the four filters on every row, and once with them in turn (tests)."""
    return ([encode(image, [kind] * image.height) for kind in (1, 2, 3, 4)]
            + [encode(image, [(y % 4) + 1 for y in range(image.height)])])
