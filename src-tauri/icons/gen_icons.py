import struct, zlib, os

def create_png(filename, size=32):
    def chunk(ctype, data):
        c = ctype + data
        return struct.pack('>I', len(data)) + c + struct.pack('>I', zlib.crc32(c) & 0xffffffff)
    header = b'\x89PNG\r\n\x1a\n'
    ihdr = chunk(b'IHDR', struct.pack('>IIBBBBB', size, size, 8, 6, 0, 0, 0))  # 6 = RGBA
    raw = b''
    for y in range(size):
        raw += b'\x00' + b'\x1a\x73\xe8' * size
    idat = chunk(b'IDAT', zlib.compress(raw))
    iend = chunk(b'IEND', b'')
    with open(filename, 'wb') as f:
        f.write(header + ihdr + idat + iend)

icons_dir = os.path.dirname(os.path.abspath(__file__))
for name, sz in [('32x32.png',32),('128x128.png',128),('128x128@2x.png',256),('icon.png',256)]:
    create_png(os.path.join(icons_dir, name), sz)
print('Icons created:', os.listdir(icons_dir))
