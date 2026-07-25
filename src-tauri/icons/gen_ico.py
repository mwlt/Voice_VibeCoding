import struct, os

# Create a minimal 32x32 ICO file
# ICO format: ICO header + 1 entry + BMP data
def create_ico(path, size=32):
    width = size
    height = size
    # 32-bit BGRA pixel data
    pixels = b''
    for y in range(height):
        for x in range(width):
            pixels += b'\xe8\x73\x1a\xff'  # BGRA blue-ish

    # BMP info header (40 bytes) + pixel data
    bmp_size = 40 + len(pixels)
    # AND mask (1 bit per pixel, row-aligned to 4 bytes)
    and_row_bytes = ((width + 31) // 32) * 4
    and_mask = b'\x00' * (and_row_bytes * height)
    bmp_total = bmp_size + len(and_mask)

    # ICO header
    ico_header = struct.pack('<HHH', 0, 1, 1)  # reserved=0, type=1(ICO), count=1

    # Directory entry
    entry = struct.pack('<BBBBHHII',
        width if width < 256 else 0,
        height if height < 256 else 0,
        0,  # color palette
        0,  # reserved
        1,  # color planes
        32, # bits per pixel
        bmp_total,
        22, # offset (6 + 16)
    )

    # BMP data: BITMAPINFOHEADER (40 bytes) + pixels + AND mask
    bmp_header = struct.pack('<IiiHHIIiiII',
        40,         # biSize
        width,      # biWidth
        height * 2, # biHeight (ICO uses doubled height for AND mask)
        1,          # biPlanes
        32,         # biBitCount
        0,          # biCompression
        len(pixels),
        0, 0, 0, 0,
    )

    with open(path, 'wb') as f:
        f.write(ico_header + entry + bmp_header + pixels + and_mask)

icons_dir = os.path.dirname(os.path.abspath(__file__))
create_ico(os.path.join(icons_dir, 'icon.ico'))
print('ICO created:', os.path.getsize(os.path.join(icons_dir, 'icon.ico')), 'bytes')
