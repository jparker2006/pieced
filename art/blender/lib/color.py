"""Colour conversions (pure Python). Palette hexes are sRGB; glTF COLOR_0 is linear."""


def srgb_to_linear(c):
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def linear_to_srgb(c):
    c = min(max(c, 0.0), 1.0)
    return c * 12.92 if c <= 0.0031308 else 1.055 * c ** (1.0 / 2.4) - 0.055


def hex_to_srgb(h):
    h = h.lstrip("#")
    if len(h) != 6:
        raise ValueError(f"bad colour hex {h!r}")
    return tuple(int(h[i:i + 2], 16) / 255.0 for i in (0, 2, 4))


def srgb_to_hex(rgb):
    return "#" + "".join(f"{min(255, max(0, round(c * 255))):02X}" for c in rgb)
