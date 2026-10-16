"""Reduces screenshots to 256 colours, which keeps them sharp at a fraction
of the size. Used by scripts/screenshots.mjs; needs Pillow."""

import sys

from PIL import Image

for path in sys.argv[1:]:
    image = Image.open(path).convert("RGB")
    image.quantize(256, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE).save(path, optimize=True)
