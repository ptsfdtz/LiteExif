"""Development-only oracle: run the user's semi-utils checkout, never shipped.

Usage: python scripts/render-reference.py C:/path/to/semi-utils
Then set LITEEXIF_PARITY_DIR to tmp/render-parity and run the ignored Rust test.
"""
import argparse
import copy
import json
from pathlib import Path
import sys
import time
import shutil
import os

parser = argparse.ArgumentParser()
parser.add_argument("upstream", type=Path)
parser.add_argument("--update-golden", action="store_true")
parser.add_argument("--benchmark", action="store_true", help="use 3000x2000 photos; do not update goldens")
args = parser.parse_args()
args.upstream = args.upstream.resolve()
if args.benchmark and args.update_golden:
    parser.error('benchmark images are not regression fixtures')
sys.path.insert(0, str(args.upstream.resolve()))
from PIL import Image
import numpy as np
from jinja2 import Template
from core.jinja2renders import auto_logo, vw, vh
from processor.core import start_process
import processor.core as pipeline

root = Path(__file__).resolve().parents[1]
os.chdir(root)
out = root / ('tmp/render-benchmark' if args.benchmark else 'tmp/render-parity')
out.mkdir(parents=True, exist_ok=True)
cases = []

def add(name, source, nodes=None, template=None, exif=None, reference_template=None):
    case = {"name": name, "input": str(source), "nodes": nodes,
            "template": template, "exif": exif}
    if template:
        parsed = Template(reference_template or template)
        parsed.globals.update(auto_logo=auto_logo, vw=vw, vh=vh)
        nodes = json.loads(parsed.render(exif=exif, filename=source.stem,
                           file_dir=source.parent.as_posix(), file_path=source.as_posix(),
                           folder_name=source.parent.name, files=[]))
    # The native pipeline receives EXIF from its caller. Exclude subprocess
    # startup here too, so timings compare rendering rather than ExifTool calls.
    pipeline.get_exif = lambda _: exif or {}
    start = time.perf_counter()
    result = start_process(copy.deepcopy(nodes), str(source))
    case["reference_ms"] = (time.perf_counter() - start) * 1000
    result.convert("RGBA").save(out / f"{name}-reference.png")
    cases.append(case)

sizes = {"landscape": (3000,2000), "portrait": (2000,3000)} if args.benchmark else {"landscape": (960,640), "portrait": (640,960)}
for shape, (w, h) in sizes.items():
    y, x = np.mgrid[:h, :w]
    pixels = np.stack([(x*255//w), (y*255//h), ((x//31+y//23)%2)*160+40], axis=-1).astype('uint8')
    source = out / f"{shape}.png"
    Image.fromarray(pixels).save(source)
    exif = {"ImageWidth": str(w), "ImageHeight": str(h), "Make": "NIKON CORPORATION",
            "CameraModelName": "NIKON Z 8", "LensModel": "NIKKOR Z 24-70mm f/2.8 S",
            "FocalLengthIn35mmFormat": "50 mm", "FNumber": "2.8", "ShutterSpeed": "1/250",
            "ISO": "100", "DateTimeOriginal": "2026-09-17 12:34:56"}
    for index, path in enumerate(sorted((root / "config/templates").glob("*.json"))):
        add(f"{shape}-{index}", source,
            template=(args.upstream / 'config/templates' / path.name).read_text('utf-8'), exif=exif)
        cases[-1]['template_name'] = path.stem

source = out / "landscape.png"
for trim in (False, True):
    for index, text in enumerate(["NIKON Z 8", "50mm f/2.8 1/250s ISO100", "照片水印 Agjp", "AV fi 2026-09-17"]):
        add(f"text-{int(trim)}-{index}", source, [{"processor_name": "rich_text", "text": text,
            "height": 37, "trim": trim, "font_path": "AlibabaPuHuiTi-2-85-Bold.otf"}])
for radius in (1, 3, 9, 35):
    add(f"blur-{radius}", source, [{"processor_name": "blur", "blur_radius": radius}])
for index, (w, h) in enumerate([(137, 91), (1111, 739)]):
    add(f"resize-{index}", source, [{"processor_name": "resize", "width": w, "height": h}])
add("round-shadow", source, [{"processor_name": "rounded_corner", "border_radius": 37},
                            {"processor_name": "shadow", "shadow_radius": 19}])
for name, options in {
    "white": {"color": "white"},
    "orange": {"color": "(232,141,52)"},
    "bold": {"is_bold": True},
    "rounding": {"height": 122},
    "missing-font": {"font_path": "missing-font.otf", "trim": True, "color": "red"},
}.items():
    add(f"text-{name}", source, [{"processor_name": "rich_text", "text": "AV Z 8 中文",
        "height": 37, "font_path": "AlibabaPuHuiTi-2-45-Light.otf", **options}])
(out / "fixtures.json").write_text(json.dumps(cases, ensure_ascii=False, indent=2), 'utf-8')
if args.update_golden:
    golden = root / 'src-tauri/tests/fixtures/render-parity'
    golden.mkdir(parents=True, exist_ok=True)
    for name in ('landscape.png', 'portrait.png'):
        shutil.copyfile(out/name,golden/name)
    portable = copy.deepcopy(cases)
    for case in portable:
        case['input'] = Path(case['input']).name
        shutil.copyfile(out/f"{case['name']}-reference.png",golden/f"{case['name']}-reference.png")
    (golden/'fixtures.json').write_text(json.dumps(portable,ensure_ascii=False,indent=2),'utf-8')
print(f"Wrote {len(cases)} reference cases to {out}")
