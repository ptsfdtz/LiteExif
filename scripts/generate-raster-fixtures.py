"""Regenerate small, independent Pillow 12.1.0 golden images for cargo test."""
from pathlib import Path
from PIL import Image, ImageFilter, ImageDraw

root = Path(__file__).resolve().parents[1] / 'src-tauri/tests/fixtures'
root.mkdir(parents=True, exist_ok=True)
source = Image.new('RGBA', (37, 23))
source.putdata([((x*17+y*29)%256, (x*x+y*11)%256, (x*7+y*y)%256,
                 (x*13+y*19+31)%256) for y in range(23) for x in range(37)])
source.save(root / 'source.png')
for w,h in [(74,46),(51,31),(19,11),(13,41)]:
    source.resize((w,h), Image.Resampling.LANCZOS).save(root / f'resize-{w}-{h}.png')
for radius in [1,3,9,35]:
    source.filter(ImageFilter.GaussianBlur(radius)).save(root / f'blur-{radius}.png')
for radius in [0,3,6,37]:
    mask=Image.new('L',source.size)
    ImageDraw.Draw(mask).rounded_rectangle([(0,0),source.size],radius,fill=255)
    result=source.copy()
    result.putalpha(mask)
    result.save(root / f'round-{radius}.png')
