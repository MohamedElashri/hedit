#!/usr/bin/env python3
"""Convert a binary PPM (P6) image to PNG with the standard library only.

Usage: python3 scripts/ppm-to-png.py input.ppm output.png
"""
import struct
import sys
import zlib


def read_ppm(path):
    data = open(path, 'rb').read()
    # Header: magic, width, height, maxval, each separated by whitespace.
    fields, index = [], 0
    while len(fields) < 4:
        while data[index:index + 1].isspace():
            index += 1
        start = index
        while not data[index:index + 1].isspace():
            index += 1
        fields.append(data[start:index])
    if fields[0] != b'P6' or fields[3] != b'255':
        sys.exit('Expected an 8-bit binary PPM (P6).')
    width, height = int(fields[1]), int(fields[2])
    return width, height, data[index + 1:index + 1 + width * height * 3]


def write_png(path, width, height, pixels):
    def chunk(kind, body):
        return (struct.pack('>I', len(body)) + kind + body
                + struct.pack('>I', zlib.crc32(kind + body) & 0xFFFFFFFF))
    stride = width * 3
    rows = b''.join(b'\x00' + pixels[y * stride:(y + 1) * stride] for y in range(height))
    with open(path, 'wb') as out:
        out.write(b'\x89PNG\r\n\x1a\n')
        out.write(chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0)))
        out.write(chunk(b'IDAT', zlib.compress(rows, 9)))
        out.write(chunk(b'IEND', b''))


if __name__ == '__main__':
    if len(sys.argv) != 3:
        sys.exit(__doc__.strip())
    write_png(sys.argv[2], *read_ppm(sys.argv[1]))
