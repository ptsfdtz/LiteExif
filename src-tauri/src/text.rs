//! Native FreeType rendering matching the Windows Pillow BASIC layout.
//! Uses the same 512px base, hinted advances and ascender baseline.
use freetype::{
    face::{KerningMode, LoadFlag},
    Library,
};
use image::{Rgba, RgbaImage};
use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    time::SystemTime,
};

struct CachedFont {
    stamp: (Option<SystemTime>, u64),
    face: freetype::Face,
}

thread_local! {
    static FONTS: RefCell<HashMap<PathBuf, CachedFont>> = RefCell::new(HashMap::new());
}

pub fn render(path: Option<&Path>, text: &str, color: Rgba<u8>) -> Result<RgbaImage, String> {
    let key = path.map(Path::to_path_buf).unwrap_or_default();
    let stamp = path
        .and_then(|p| fs::metadata(p).ok())
        .map(|m| (m.modified().ok(), m.len()))
        .unwrap_or((None, 0));
    FONTS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.get(&key).is_none_or(|font| font.stamp != stamp) {
            let library = Library::init().map_err(|e| e.to_string())?;
            let requested = path
                .and_then(|p| fs::read(p).ok())
                .and_then(|bytes| library.new_memory_face(Rc::new(bytes), 0).ok());
            let (face, size) = if let Some(face) = requested {
                (face, 512)
            } else {
                // Pillow load_font catches an invalid explicit font and uses
                // its built-in 10px Aileron face, not the normal 512px font.
                (
                    library
                        .new_memory_face(include_bytes!("../assets/pillow-default.ttf").to_vec(), 0)
                        .map_err(|e| e.to_string())?,
                    10,
                )
            };
            face.set_pixel_sizes(0, size).map_err(|e| e.to_string())?;
            if cache.len() >= 16 {
                cache.clear();
            }
            cache.insert(key.clone(), CachedFont { stamp, face });
        }
        render_face(&cache[&key], text, color)
    })
}

struct PositionedGlyph {
    x: i64,
    left: i32,
    top: i32,
    width: usize,
    rows: usize,
    pitch: usize,
    buffer: Vec<u8>,
}

fn render_face(font: &CachedFont, text: &str, color: Rgba<u8>) -> Result<RgbaImage, String> {
    let face = &font.face;
    let pixel = |n: i64| (n + 32) >> 6;
    let metrics = face.size_metrics().ok_or("无法读取字体度量")?;
    let ascent = pixel(metrics.ascender as i64);
    let height = ascent - pixel(metrics.descender as i64);
    let mut pen = 0i64;
    let mut left = 0i64;
    let mut right = 0i64;
    // Load each glyph with rendering enabled once. The bitmap is copied out of
    // the reusable FreeType slot so bounds and pixels come from a single load.
    let mut glyphs = Vec::new();
    let mut previous = 0;
    for character in text.chars() {
        let id = face.get_char_index(character as usize).unwrap_or(0);
        if previous != 0 && id != 0 && face.has_kerning() {
            let delta = face
                .get_kerning(previous, id, KerningMode::KerningDefault)
                .map_err(|e| e.to_string())?;
            // Preserve Pillow BASIC's 26.6 kerning quantization.
            pen += pixel(delta.x as i64);
        }
        let x = pixel(pen);
        face.load_glyph(id, LoadFlag::RENDER)
            .map_err(|e| e.to_string())?;
        let slot = face.glyph();
        // Copy the rendered bitmap before `get_glyph`, whose temporary
        // `FT_Glyph` can release the memory backing the slot's bitmap.
        let bitmap = slot.bitmap();
        let width = bitmap.width() as usize;
        let rows = bitmap.rows() as usize;
        let pitch = bitmap.pitch().unsigned_abs() as usize;
        let buffer = if width > 0 && rows > 0 {
            bitmap.buffer().to_vec()
        } else {
            Vec::new()
        };
        let bounds = slot
            .get_glyph()
            .map_err(|e| e.to_string())?
            .get_cbox(freetype::ffi::FT_GLYPH_BBOX_PIXELS);
        left = left.min(x + bounds.xMin as i64);
        right = right.max(x + bounds.xMax as i64);
        pen += slot.metrics().horiAdvance as i64;
        right = right.max(pixel(pen));
        glyphs.push(PositionedGlyph {
            x,
            left: slot.bitmap_left(),
            top: slot.bitmap_top(),
            width,
            rows,
            pitch,
            buffer,
        });
        previous = id;
    }
    let mut image = RgbaImage::new((right - left).max(1) as u32, height.max(1) as u32);
    for glyph in glyphs {
        for row in 0..glyph.rows {
            for col in 0..glyph.width {
                let px = glyph.x + glyph.left as i64 + col as i64;
                let py = ascent - glyph.top as i64 + row as i64;
                if px < 0 || py < 0 || px >= image.width() as i64 || py >= image.height() as i64 {
                    continue;
                }
                let coverage = glyph.buffer[row * glyph.pitch + col] as u32;
                if coverage == 0 {
                    continue;
                }
                let target = image.get_pixel_mut(px as u32, py as u32);
                // Pillow's RGBA text ink is straight (not premultiplied) RGB.
                for c in 0..3 {
                    target[c] = color[c];
                }
                target[3] = (coverage + (target[3] as u32 * (255 - coverage) + 127) / 255) as u8;
            }
        }
    }
    for pixel in image.pixels_mut() {
        pixel[3] = ((pixel[3] as u32 * color[3] as u32 + 127) / 255) as u8;
    }
    Ok(image)
}
