#!/usr/bin/env python3
"""Verify that the headless Lua worker has no GUI/profile DLL startup imports."""
import argparse
import json
from pathlib import Path
import struct


def inspect(path):
    data = path.read_bytes()
    if data[:2] != b'MZ':
        raise ValueError('Expected a Windows executable.')
    pe = struct.unpack_from('<I', data, 60)[0]
    if data[pe:pe + 4] != b'PE\0\0':
        raise ValueError('Invalid PE signature.')
    machine, count = struct.unpack_from('<HH', data, pe + 4)
    optional_size = struct.unpack_from('<H', data, pe + 20)[0]
    optional = pe + 24
    if machine not in (0x8664, 0xaa64) or struct.unpack_from('<H', data, optional)[0] != 0x20b:
        raise ValueError('Expected a 64-bit x86 or ARM executable.')
    sections = []
    for index in range(count):
        virtual_size, address, raw_size, raw = struct.unpack_from('<IIII', data, optional + optional_size + index * 40 + 8)
        sections.append((address, max(virtual_size, raw_size), raw))

    def offset(rva):
        for address, size, raw in sections:
            if address <= rva < address + size:
                result = raw + rva - address
                if result < len(data):
                    return result
        raise ValueError('Import address is outside PE sections.')

    imports = []
    rva, size = struct.unpack_from('<II', data, optional + 112 + 8)
    if rva:
        start = offset(rva)
        for index in range(min(size // 20, 1024)):
            entry = struct.unpack_from('<IIIII', data, start + index * 20)
            if not any(entry):
                break
            name = offset(entry[3])
            end = data.index(b'\0', name, min(len(data), name + 260))
            imports.append(data[name:end].decode('ascii').lower())
        else:
            raise ValueError('Import directory has no bounded terminator.')
    forbidden = {'user32.dll', 'gdi32.dll', 'shell32.dll', 'ole32.dll', 'userenv.dll'}
    unexpected = forbidden.intersection(imports)
    if unexpected:
        raise ValueError('Parent-only GUI/profile DLLs are imported at worker startup: ' + ', '.join(sorted(unexpected)))
    return {'binary': str(path), 'bytes': len(data), 'imports': sorted(imports)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path)
    args = parser.parse_args()
    print(json.dumps(inspect(args.binary), indent=2))


if __name__ == '__main__':
    main()
