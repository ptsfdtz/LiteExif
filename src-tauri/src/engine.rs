use image::imageops;
use image::{DynamicImage, ImageDecoder, ImageEncoder, ImageReader, Rgba, RgbaImage};
use minijinja::{context, Environment, Error as TemplateError, State, Value as TemplateValue};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU8, Ordering};
use walkdir::WalkDir;

pub type EngineResult<T> = Result<T, String>;

#[derive(Clone, Debug)]
pub struct IniConfig {
    sections: HashMap<String, HashMap<String, String>>,
}

impl IniConfig {
    pub fn load(path: &Path) -> EngineResult<Self> {
        let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
        let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut section = "DEFAULT".to_owned();
        for raw_line in content.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                section = line[1..line.len() - 1].to_owned();
            } else if let Some((key, value)) = line.split_once('=') {
                sections
                    .entry(section.clone())
                    .or_default()
                    .insert(key.trim().to_owned(), value.trim().to_owned());
            }
        }
        Ok(Self { sections })
    }

    pub fn get(&self, section: &str, key: &str) -> EngineResult<String> {
        self.sections
            .get(section)
            .and_then(|values| values.get(key))
            .cloned()
            .ok_or_else(|| format!("配置项不存在: {section}.{key}"))
    }

    pub fn get_bool(&self, section: &str, key: &str) -> EngineResult<bool> {
        Ok(matches!(
            self.get(section, key)?.to_ascii_lowercase().as_str(),
            "true" | "1" | "yes" | "on"
        ))
    }

    pub fn get_i64(&self, section: &str, key: &str) -> EngineResult<i64> {
        self.get(section, key)?
            .parse::<i64>()
            .map_err(|error| error.to_string())
    }

    pub fn set(&mut self, section: &str, key: &str, value: impl ToString) {
        self.sections
            .entry(section.to_owned())
            .or_default()
            .insert(key.to_owned(), value.to_string());
    }

    pub fn save(&self, path: &Path) -> EngineResult<()> {
        let mut output = String::new();
        for section in ["DEFAULT", "render"] {
            if let Some(values) = self.sections.get(section) {
                output.push_str(&format!("[{section}]\n"));
                let order: &[&str] = if section == "DEFAULT" {
                    &[
                        "host",
                        "port",
                        "debug",
                        "input_folder",
                        "output_folder",
                        "override_existed",
                        "supported_file_suffixes",
                        "quality",
                        "subsampling",
                    ]
                } else {
                    &["template_name"]
                };
                for key in order {
                    if let Some(value) = values.get(*key) {
                        output.push_str(&format!("{key} = {value}\n"));
                    }
                }
                for (key, value) in values {
                    if !order.contains(&key.as_str()) {
                        output.push_str(&format!("{key} = {value}\n"));
                    }
                }
                output.push('\n');
            }
        }
        fs::write(path, output).map_err(|error| error.to_string())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct FileNode {
    pub label: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_file: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<FileNode>>,
}

/// Lists one directory level, sorted (sub-directories first, then files by
/// modification time). Results are paged so a folder with tens of thousands of
/// entries never produces a single huge payload. Returns the page and whether
/// more entries remain. Directories are returned without `children`; the
/// frontend requests each level on demand (lazy loading), which keeps network
/// folders such as NAS mounts responsive.
pub fn list_directory(
    path: &Path,
    suffixes: &[String],
    offset: usize,
    limit: usize,
) -> (Vec<FileNode>, bool) {
    if !path.exists() {
        return (Vec::new(), false);
    }
    let mut directories = Vec::new();
    let mut files = Vec::new();
    let Ok(entries) = fs::read_dir(path) else {
        return (Vec::new(), false);
    };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || entry_path.is_symlink() {
            continue;
        }
        if entry_path.is_dir() {
            directories.push((name, entry_path));
        } else if entry_path.is_file() {
            files.push((name, entry_path));
        }
    }
    directories.sort_by(|a, b| b.0.to_lowercase().cmp(&a.0.to_lowercase()));
    files.sort_by(|a, b| {
        // On Windows the modification time comes from the directory
        // enumeration, so this stays cheap even on network shares.
        let a_time = a.1.metadata().and_then(|meta| meta.modified()).ok();
        let b_time = b.1.metadata().and_then(|meta| meta.modified()).ok();
        b_time
            .cmp(&a_time)
            .then_with(|| b.0.to_lowercase().cmp(&a.0.to_lowercase()))
    });
    let mut items = Vec::with_capacity(directories.len() + files.len());
    for (name, child_path) in directories {
        items.push(FileNode {
            label: name,
            value: child_path.to_string_lossy().into_owned(),
            is_file: None,
            children: None,
        });
    }
    for (name, file_path) in files {
        let suffix = file_path
            .extension()
            .map(|value| format!(".{}", value.to_string_lossy().to_ascii_lowercase()))
            .unwrap_or_default();
        if suffixes.iter().any(|value| value == &suffix) {
            items.push(FileNode {
                label: name,
                value: file_path.to_string_lossy().into_owned(),
                is_file: Some(true),
                children: None,
            });
        }
    }
    let total = items.len();
    let page: Vec<FileNode> = items.into_iter().skip(offset).take(limit).collect();
    let has_more = offset + page.len() < total;
    (page, has_more)
}

pub fn list_templates(root: &Path) -> Vec<String> {
    let mut templates: Vec<String> = fs::read_dir(root.join("config/templates"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path
                .extension()?
                .to_string_lossy()
                .eq_ignore_ascii_case("json")
            {
                path.file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
            } else {
                None
            }
        })
        .collect();
    templates.sort();
    templates
}

pub fn get_exif(root: &Path, path: &Path) -> HashMap<String, String> {
    let executable = root.join("exiftool/exiftool.exe");
    let Ok(output) = Command::new(executable)
        .args(["-d", "%Y-%m-%d %H:%M:%S%3f%z"])
        .arg(path)
        .output()
    else {
        return HashMap::new();
    };
    parse_exif(&String::from_utf8_lossy(&output.stdout))
}

/// Keep template-relative dimensions in the same coordinate system as the
/// decoded pixels. Cameras commonly store portrait JPEGs as landscape pixels
/// plus an EXIF rotation, while `load_image` applies that rotation before the
/// processing pipeline runs.
pub fn normalize_exif_dimensions(exif: &mut HashMap<String, String>, image: &RgbaImage) {
    exif.insert("ImageWidth".to_owned(), image.width().to_string());
    exif.insert("ImageHeight".to_owned(), image.height().to_string());
}

fn parse_exif(text: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let clean_key = key
                .chars()
                .filter(|ch| !ch.is_whitespace() && *ch != '/')
                .collect::<String>();
            let clean_value = value
                .trim()
                .chars()
                .filter(char::is_ascii)
                .collect::<String>();
            result.insert(clean_key, clean_value);
        }
    }
    // Preserve upstream field names and missing values. Inventing aliases
    // changes the visible watermark for cameras with incomplete EXIF.
    result
}

fn template_number(state: &State<'_, '_>, key: &str) -> i64 {
    state
        .lookup("exif")
        .and_then(|exif| exif.get_item(&TemplateValue::from(key)).ok())
        .and_then(|value| {
            value
                .as_str()
                .and_then(|text| text.parse::<i64>().ok())
                .or_else(|| value.as_i64())
        })
        .unwrap_or(0)
}

pub fn render_template(
    root: &Path,
    source: &str,
    exif: &HashMap<String, String>,
    file_path: &Path,
    files: &[String],
) -> EngineResult<String> {
    let mut environment = Environment::new();
    environment.set_unknown_method_callback(|state, value, method, args| {
        if let (Some(text), "partition") = (value.as_str(), method) {
            let (separator,): (&str,) = minijinja::value::from_args(args)?;
            if separator.is_empty() {
                return Err(TemplateError::new(
                    minijinja::ErrorKind::InvalidOperation,
                    "empty separator",
                ));
            }
            let parts = match text.split_once(separator) {
                Some((left, right)) => vec![left, separator, right],
                None => vec![text, "", ""],
            };
            return Ok(TemplateValue::from(
                parts.into_iter().map(str::to_owned).collect::<Vec<_>>(),
            ));
        }
        minijinja_contrib::pycompat::unknown_method_callback(state, value, method, args)
    });
    environment.add_function("vw", |state: &State<'_, '_>, percent: f64| -> i64 {
        (template_number(state, "ImageWidth") as f64 * percent / 100.0) as i64
    });
    environment.add_function("vh", |state: &State<'_, '_>, percent: f64| -> i64 {
        (template_number(state, "ImageHeight") as f64 * percent / 100.0) as i64
    });
    let logos = root.join("config/logos");
    environment.add_function(
        "auto_logo",
        move |state: &State<'_, '_>, brand: Option<String>| -> Result<String, TemplateError> {
            let make = brand
                .or_else(|| {
                    state
                        .lookup("exif")
                        .and_then(|value| value.get_item(&TemplateValue::from("Make")).ok())
                        .and_then(|value| value.as_str().map(str::to_owned))
                })
                .unwrap_or_else(|| "default".to_owned())
                .to_lowercase();
            for entry in fs::read_dir(&logos).into_iter().flatten().flatten() {
                let path = entry.path();
                let stem = path
                    .file_stem()
                    .map(|value| value.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                let extension = path
                    .extension()
                    .map(|value| value.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if ["png", "jpg", "jpeg"].contains(&extension.as_str()) && make.contains(&stem) {
                    return Ok(path.to_string_lossy().replace('\\', "/"));
                }
            }
            Ok(String::new())
        },
    );
    environment.add_filter(
        "partition",
        |text: String, separator: String| -> Vec<String> {
            if let Some((left, right)) = text.split_once(&separator) {
                vec![left.to_owned(), separator, right.to_owned()]
            } else {
                vec![text, String::new(), String::new()]
            }
        },
    );
    environment
        .add_template("template", source)
        .map_err(|error| error.to_string())?;
    let template = environment
        .get_template("template")
        .map_err(|error| error.to_string())?;
    template.render(context! {
        exif => exif,
        filename => file_path.file_stem().unwrap_or_default().to_string_lossy().into_owned(),
        file_dir => file_path.parent().unwrap_or(Path::new("")).to_string_lossy().replace('\\', "/"),
        folder_name => file_path.parent().and_then(Path::file_name).unwrap_or_default().to_string_lossy().into_owned(),
        file_path => file_path.to_string_lossy().replace('\\', "/"),
        files => files,
    }).map_err(|error| error.to_string())
}

fn value_i64(node: &Value, key: &str, default: i64) -> i64 {
    node.get(key)
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_f64().map(|number| number as i64))
                .or_else(|| value.as_str()?.parse().ok())
        })
        .unwrap_or(default)
}
fn value_f64(node: &Value, key: &str, default: f64) -> f64 {
    node.get(key)
        .and_then(|value| {
            value
                .as_f64()
                .or_else(|| value.as_i64().map(|number| number as f64))
                .or_else(|| value.as_str()?.parse().ok())
        })
        .unwrap_or(default)
}
fn value_bool(node: &Value, key: &str, default: bool) -> bool {
    node.get(key)
        .and_then(|value| {
            value
                .as_bool()
                .or_else(|| value.as_str().map(|text| text.eq_ignore_ascii_case("true")))
        })
        .unwrap_or(default)
}
fn value_string(node: &Value, key: &str, default: &str) -> String {
    node.get(key)
        .and_then(Value::as_str)
        .unwrap_or(default)
        .to_owned()
}
fn parse_json_array(node: &Value, key: &str) -> Vec<Value> {
    match node.get(key) {
        Some(Value::Array(values)) => values.clone(),
        Some(Value::String(text)) => serde_json::from_str(text).unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn parse_color(value: &Value, default: Rgba<u8>) -> Rgba<u8> {
    if let Some(array) = value.as_array() {
        let mut channels = [0u8; 4];
        for (index, channel) in array.iter().take(4).enumerate() {
            channels[index] = channel.as_u64().unwrap_or(0) as u8;
        }
        if array.len() == 3 {
            channels[3] = 255;
        }
        return Rgba(channels);
    }
    let Some(text) = value.as_str() else {
        return default;
    };
    let trimmed = text.trim().trim_matches(['(', ')']);
    if trimmed.contains(',') {
        let values: Vec<u8> = trimmed
            .split(',')
            .filter_map(|part| part.trim().parse().ok())
            .collect();
        if values.len() == 3 {
            return Rgba([values[0], values[1], values[2], 255]);
        }
        if values.len() == 4 {
            return Rgba([values[0], values[1], values[2], values[3]]);
        }
    }
    if let Some(hex) = text.strip_prefix('#') {
        if (hex.len() == 6 || hex.len() == 8) && hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
            let r = u8::from_str_radix(&hex[0..2], 16).unwrap();
            let g = u8::from_str_radix(&hex[2..4], 16).unwrap();
            let b = u8::from_str_radix(&hex[4..6], 16).unwrap();
            let a = if hex.len() == 8 {
                u8::from_str_radix(&hex[6..8], 16).unwrap()
            } else {
                255
            };
            return Rgba([r, g, b, a]);
        }
    }
    match text.to_ascii_lowercase().as_str() {
        "black" => Rgba([0, 0, 0, 255]),
        "white" => Rgba([255, 255, 255, 255]),
        "red" => Rgba([255, 0, 0, 255]),
        "blue" => Rgba([0, 0, 255, 255]),
        "green" => Rgba([0, 128, 0, 255]),
        "transparent" => Rgba([0, 0, 0, 0]),
        _ => default,
    }
}

/// Mirrors Pillow's `Image.paste(source, box, mask=source)` behavior.
///
/// Pillow interpolates every RGBA channel with the source alpha mask.  This
/// differs from Porter-Duff source-over for translucent pixels, and all
/// template composition in the original Python implementation uses `paste`.
fn alpha_over(canvas: &mut RgbaImage, image: &RgbaImage, x: i64, y: i64) {
    for source_y in 0..image.height() {
        for source_x in 0..image.width() {
            let target_x = x + source_x as i64;
            let target_y = y + source_y as i64;
            if target_x < 0
                || target_y < 0
                || target_x >= canvas.width() as i64
                || target_y >= canvas.height() as i64
            {
                continue;
            }
            let source = image.get_pixel(source_x, source_y).0;
            let target = canvas.get_pixel(target_x as u32, target_y as u32).0;
            let mut output = [0u8; 4];
            let mask = source[3] as u32;
            for channel in 0..4 {
                // Integer rounding is deliberately used here: it matches the
                // Pillow compositing path used by the Python reference.
                output[channel] =
                    ((source[channel] as u32 * mask + target[channel] as u32 * (255 - mask) + 127)
                        / 255) as u8;
            }
            canvas.put_pixel(target_x as u32, target_y as u32, Rgba(output));
        }
    }
}

fn resize_image(
    image: &RgbaImage,
    width: Option<u32>,
    height: Option<u32>,
    scale: Option<f64>,
) -> RgbaImage {
    let (target_width, target_height) = match (width, height, scale) {
        (Some(w), Some(h), _) => (w, h),
        (Some(w), None, _) => {
            let factor = w as f64 / image.width() as f64;
            (
                (image.width() as f64 * factor) as u32,
                (image.height() as f64 * factor) as u32,
            )
        }
        (None, Some(h), _) => {
            let factor = h as f64 / image.height() as f64;
            (
                (image.width() as f64 * factor) as u32,
                (image.height() as f64 * factor) as u32,
            )
        }
        (_, _, Some(factor)) => (
            (image.width() as f64 * factor) as u32,
            (image.height() as f64 * factor) as u32,
        ),
        _ => image.dimensions(),
    };
    resize_lanczos_parallel(image, target_width.max(1), target_height.max(1))
}

fn resize_lanczos_parallel(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    crate::raster::resize(image, width, height)
}

fn gaussian_blur_cpu(image: &RgbaImage, radius: u32) -> RgbaImage {
    crate::raster::gaussian_blur(image, radius)
}

// GPU output is accepted only after a byte-for-byte comparison with the CPU
// implementation. Both preview and export use this shared entry point.
static GPU_BLUR_STATE: AtomicU8 = AtomicU8::new(0);
static GPU_BLUR_VALIDATION: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
const GPU_VALIDATED: u8 = 1;
const GPU_DISABLED: u8 = 2;

fn gaussian_blur(image: &RgbaImage, radius: i64) -> RgbaImage {
    let radius = radius.max(0) as u32;
    if radius == 0 || image.width() == 0 || image.height() == 0 {
        return image.clone();
    }

    // Dispatch overhead dominates tiny layers; camera-image backgrounds and
    // their 512 px previews comfortably exceed this threshold.
    let eligible = image.width() as u64 * image.height() as u64 >= 128 * 1024 && radius >= 2;
    if !eligible || GPU_BLUR_STATE.load(Ordering::Acquire) == GPU_DISABLED {
        return gaussian_blur_cpu(image, radius);
    }

    if GPU_BLUR_STATE.load(Ordering::Acquire) == GPU_VALIDATED || initialize_gpu_acceleration() {
        return match crate::gpu::blur(image, radius) {
            Ok((output, _)) => output,
            Err(error) => {
                eprintln!("LiteExif GPU blur disabled after runtime failure: {error}");
                GPU_BLUR_STATE.store(GPU_DISABLED, Ordering::Release);
                gaussian_blur_cpu(image, radius)
            }
        };
    }

    gaussian_blur_cpu(image, radius)
}

pub fn initialize_gpu_acceleration() -> bool {
    *GPU_BLUR_VALIDATION.get_or_init(|| {
        let mut source = RgbaImage::new(37, 23);
        for (x, y, pixel) in source.enumerate_pixels_mut() {
            *pixel = Rgba([
                ((x * 17 + y * 29) % 256) as u8,
                ((x * x + y * 11) % 256) as u8,
                ((x * 7 + y * y) % 256) as u8,
                ((x * 13 + y * 19 + 31) % 256) as u8,
            ]);
        }
        for radius in [1, 3, 9] {
            let cpu = gaussian_blur_cpu(&source, radius);
            match crate::gpu::blur(&source, radius) {
                Ok((gpu, adapter)) if gpu.as_raw() == cpu.as_raw() => {
                    eprintln!("LiteExif GPU blur check passed on {adapter}, radius {radius}");
                }
                Ok((_gpu, adapter)) => {
                    eprintln!("LiteExif GPU blur disabled: output mismatch on {adapter}");
                    GPU_BLUR_STATE.store(GPU_DISABLED, Ordering::Release);
                    return false;
                }
                Err(error) => {
                    eprintln!("LiteExif GPU blur unavailable; using CPU: {error}");
                    GPU_BLUR_STATE.store(GPU_DISABLED, Ordering::Release);
                    return false;
                }
            }
        }
        GPU_BLUR_STATE.store(GPU_VALIDATED, Ordering::Release);
        true
    })
}

pub fn gpu_acceleration_status() -> (&'static str, Option<&'static str>) {
    let state = match GPU_BLUR_STATE.load(Ordering::Acquire) {
        GPU_VALIDATED => "validated",
        GPU_DISABLED => "disabled",
        _ => "pending",
    };
    (state, crate::gpu::adapter_name())
}

fn foreground_bbox(
    image: &RgbaImage,
    trim_left: bool,
    trim_right: bool,
    trim_top: bool,
    trim_bottom: bool,
) -> (u32, u32, u32, u32) {
    let corners = [
        image.get_pixel(0, 0).0,
        image.get_pixel(image.width() - 1, 0).0,
        image.get_pixel(0, image.height() - 1).0,
        image.get_pixel(image.width() - 1, image.height() - 1).0,
    ];
    let mut background = [0f32; 4];
    for corner in corners {
        for channel in 0..4 {
            background[channel] += corner[channel] as f32 / 4.0;
        }
    }
    let different = |pixel: &Rgba<u8>| -> bool {
        pixel
            .0
            .iter()
            .enumerate()
            .map(|(index, value)| (*value as f32 - background[index]).powi(2))
            .sum::<f32>()
            .sqrt()
            > 10.0
    };
    let mut left = 0;
    let mut right = image.width();
    let mut top = 0;
    let mut bottom = image.height();
    if trim_left {
        left = (0..image.width())
            .find(|x| (0..image.height()).any(|y| different(image.get_pixel(*x, y))))
            .unwrap_or(0);
    }
    if trim_right {
        right = (0..image.width())
            .rev()
            .find(|x| (0..image.height()).any(|y| different(image.get_pixel(*x, y))))
            .map(|x| x + 1)
            .unwrap_or(image.width());
    }
    if trim_top {
        top = (0..image.height())
            .find(|y| (0..image.width()).any(|x| different(image.get_pixel(x, *y))))
            .unwrap_or(0);
    }
    if trim_bottom {
        bottom = (0..image.height())
            .rev()
            .find(|y| (0..image.width()).any(|x| different(image.get_pixel(x, *y))))
            .map(|y| y + 1)
            .unwrap_or(image.height());
    }
    (left, top, right.max(left + 1), bottom.max(top + 1))
}

fn load_font(root: &Path, requested: Option<&str>) -> EngineResult<Option<PathBuf>> {
    let requested_path = requested.map(PathBuf::from).map(|path| {
        if path.is_absolute() {
            path
        } else {
            // The Python templates historically use both `Foo.otf` and
            // `fonts/Foo.otf`; both are relative to config/fonts.
            let path = path.strip_prefix("fonts").unwrap_or(&path);
            root.join("config/fonts").join(path)
        }
    });
    if requested.is_some_and(|value| !value.is_empty()) {
        return Ok(requested_path.filter(|path| path.is_file()));
    }
    for path in [
        root.join("config/fonts/AlibabaPuHuiTi-2-45-Light.otf"),
        PathBuf::from("C:/Windows/Fonts/arial.ttf"),
    ] {
        if path.is_file() {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn generate_text(root: &Path, node: &Value) -> EngineResult<RgbaImage> {
    let text = value_string(node, "text", " ");
    let text = if text.is_empty() {
        " ".to_owned()
    } else {
        text
    };
    let font_path = node.get("font_path").and_then(Value::as_str);
    let font = load_font(root, font_path)?;
    let color = parse_color(
        node.get("color")
            .unwrap_or(&Value::String("black".to_owned())),
        Rgba([0, 0, 0, 255]),
    );
    let mut image = crate::text::render(font.as_deref(), &text, color)?;
    let trim = value_bool(node, "trim", false);
    // Upstream always trims left/right; `trim` only controls top/bottom.
    let (left, top, right, bottom) = foreground_bbox(&image, true, true, trim, trim);
    image = imageops::crop_imm(&image, left, top, right - left, bottom - top).to_image();
    let requested_height = value_i64(node, "height", 100) as f64;
    let target_height = if value_bool(node, "is_bold", false) {
        requested_height * 1.13
    } else {
        requested_height
    };
    let factor = target_height / image.height() as f64;
    Ok(resize_image(
        &image,
        // Pillow computes the width from the fractional bold height before
        // truncating either dimension (e.g. 22 * 1.13 = 24.86, not 24).
        Some((image.width() as f64 * factor).max(1.0) as u32),
        Some((image.height() as f64 * factor).max(1.0) as u32),
        None,
    ))
}

fn concat(
    images: &[RgbaImage],
    direction: &str,
    alignment: &str,
    spacing: i64,
    background: Rgba<u8>,
) -> RgbaImage {
    if images.is_empty() {
        return RgbaImage::new(1, 1);
    }
    let horizontal = direction == "horizontal";
    let width = if horizontal {
        images.iter().map(RgbaImage::width).sum::<u32>()
            + spacing.max(0) as u32 * (images.len() as u32 - 1)
    } else {
        images.iter().map(RgbaImage::width).max().unwrap()
    };
    let height = if horizontal {
        images.iter().map(RgbaImage::height).max().unwrap()
    } else {
        images.iter().map(RgbaImage::height).sum::<u32>()
            + spacing.max(0) as u32 * (images.len() as u32 - 1)
    };
    let mut canvas = RgbaImage::from_pixel(width, height, background);
    let mut cursor = 0i64;
    for image in images {
        let offset = |size: u32, maximum: u32| -> i64 {
            match alignment {
                "center" | "middle" => (maximum - size) as i64 / 2,
                "end" | "right" | "bottom" => (maximum - size) as i64,
                _ => 0,
            }
        };
        let (x, y) = if horizontal {
            (cursor, offset(image.height(), height))
        } else {
            (offset(image.width(), width), cursor)
        };
        alpha_over(&mut canvas, image, x, y);
        cursor += if horizontal {
            image.width() as i64 + spacing
        } else {
            image.height() as i64 + spacing
        };
    }
    canvas
}

fn generate_multi_text(root: &Path, node: &Value) -> EngineResult<RgbaImage> {
    let height = value_i64(node, "height", 100);
    let mut images = Vec::new();
    for segment in node
        .get("text_segments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let mut segment = segment;
        segment
            .as_object_mut()
            .unwrap()
            .insert("height".to_owned(), json!(height));
        images.push(generate_text(root, &segment)?);
    }
    Ok(concat(
        &images,
        "horizontal",
        &value_string(node, "text_alignment", "bottom"),
        value_i64(node, "text_spacing", 0),
        Rgba([255, 255, 255, 0]),
    ))
}

fn alignment(mut images: Vec<RgbaImage>, node: &Value) -> RgbaImage {
    if images.is_empty() {
        return RgbaImage::new(1, 1);
    }
    let weights = parse_json_array(node, "weights");
    if !weights.is_empty() {
        let mut weighted: Vec<(i64, RgbaImage)> = images
            .into_iter()
            .enumerate()
            .map(|(index, image)| {
                (
                    weights.get(index).and_then(Value::as_i64).unwrap_or(0),
                    image,
                )
            })
            .collect();
        weighted.sort_by_key(|item| item.0);
        images = weighted.into_iter().map(|item| item.1).collect();
    }
    let width = images.iter().map(RgbaImage::width).max().unwrap();
    let height = images.iter().map(RgbaImage::height).max().unwrap();
    let background = parse_color(
        node.get("background").unwrap_or(&json!([255, 255, 255, 0])),
        Rgba([255, 255, 255, 0]),
    );
    let mut canvas = RgbaImage::from_pixel(width, height, background);
    let offsets = parse_json_array(node, "offsets");
    let horizontal = value_string(node, "horizontal_alignment", "center");
    let vertical = value_string(node, "vertical_alignment", "center");
    let axis = |size: u32, maximum: u32, mode: &str| -> i64 {
        match mode {
            "center" | "middle" => (maximum - size) as i64 / 2,
            "end" | "right" | "bottom" => (maximum - size) as i64,
            _ => 0,
        }
    };
    for (index, image) in images.iter().enumerate() {
        let pair = offsets.get(index).and_then(Value::as_array);
        let offset_x = pair
            .and_then(|p| p.first())
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let offset_y = pair
            .and_then(|p| p.get(1))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        alpha_over(
            &mut canvas,
            image,
            axis(image.width(), width, &horizontal) - offset_x,
            axis(image.height(), height, &vertical) - offset_y,
        );
    }
    canvas
}

fn add_margin(
    image: &RgbaImage,
    left: i64,
    right: i64,
    top: i64,
    bottom: i64,
    color: Rgba<u8>,
) -> RgbaImage {
    let mut canvas = RgbaImage::from_pixel(
        (image.width() as i64 + left + right).max(1) as u32,
        (image.height() as i64 + top + bottom).max(1) as u32,
        color,
    );
    imageops::replace(&mut canvas, image, left, top);
    canvas
}

/// Pillow's `Image.crop` retains the requested geometry when the crop extends
/// outside the source and fills the uncovered area with the mode's zero value.
fn crop_with_padding(image: &RgbaImage, left: i64, top: i64, width: u32, height: u32) -> RgbaImage {
    let mut output = RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let source_x = left + x as i64;
            let source_y = top + y as i64;
            if source_x >= 0
                && source_y >= 0
                && source_x < image.width() as i64
                && source_y < image.height() as i64
            {
                output.put_pixel(x, y, *image.get_pixel(source_x as u32, source_y as u32));
            }
        }
    }
    output
}

fn rounded_corner(image: &RgbaImage, radius: i64) -> RgbaImage {
    crate::raster::rounded_corner(image, radius)
}

fn shadow(image: &RgbaImage, radius: i64, color: Rgba<u8>) -> RgbaImage {
    if radius <= 0 {
        return image.clone();
    }
    let padding = radius * 2;
    let mut layer = RgbaImage::from_pixel(
        image.width() + padding as u32 * 2,
        image.height() + padding as u32 * 2,
        Rgba([0, 0, 0, 0]),
    );
    for (x, y, pixel) in image.enumerate_pixels() {
        let mut shadow_pixel = color;
        // Pillow's `putalpha(original.getchannel('A'))` replaces, rather
        // than multiplies, the configured shadow alpha.
        shadow_pixel.0[3] = pixel.0[3];
        layer.put_pixel(x + padding as u32, y + padding as u32, shadow_pixel);
    }
    let mut layer = gaussian_blur(&layer, radius);
    for pixel in layer.pixels_mut() {
        let alpha = pixel.0[3] as f32 / 255.0;
        pixel.0[3] = if alpha.powf(1.5) < 0.01 {
            0
        } else {
            (alpha.powf(1.5) * 255.0) as u8
        };
    }
    alpha_over(&mut layer, image, padding, padding);
    layer
}

fn watermark(root: &Path, image: &RgbaImage, node: &Value) -> EngineResult<RgbaImage> {
    let color = parse_color(
        node.get("color").unwrap_or(&json!("white")),
        Rgba([255, 255, 255, 255]),
    );
    let delimiter_color = parse_color(
        node.get("delimiter_color").unwrap_or(&json!("black")),
        Rgba([0, 0, 0, 255]),
    );
    let delimiter_width = value_i64(
        node,
        "delimiter_width",
        (image.width() as f64 * 0.003) as i64,
    );
    let left_margin = value_i64(node, "left_margin", 0);
    let right_margin = value_i64(node, "right_margin", 0);
    let top_margin = value_i64(node, "top_margin", 0);
    let bottom_margin = value_i64(node, "bottom_margin", (image.height() as f64 * 0.12) as i64);
    let middle_spacing = value_i64(node, "middle_spacing", (bottom_margin as f64 * 0.05) as i64);
    let mut parts = Vec::new();
    for key in ["left_top", "left_bottom", "right_top", "right_bottom"] {
        let mut child = node
            .get(key)
            .cloned()
            .unwrap_or_else(|| json!({"processor_name":"rich_text"}));
        if child.get("height").is_none() {
            child.as_object_mut().unwrap().insert(
                "height".to_owned(),
                json!((bottom_margin as f64 * 0.3) as i64),
            );
        }
        let image = match child
            .get("processor_name")
            .and_then(Value::as_str)
            .unwrap_or("rich_text")
        {
            "multi_rich_text" => generate_multi_text(root, &child)?,
            _ => generate_text(root, &child)?,
        };
        parts.push(image);
    }
    let [left_top, left_bottom, right_top, right_bottom] = parts.try_into().unwrap();
    let width = (image.width() as i64 + left_margin + right_margin) as u32;
    let height = (image.height() as i64 + top_margin + bottom_margin) as u32;
    let mut canvas = RgbaImage::from_pixel(width, height, color);
    alpha_over(&mut canvas, image, left_margin, top_margin);
    let footer_y = top_margin + image.height() as i64;
    let common_spacing = (width as f64 * 0.02) as i64;
    let mut left_logo_width = 0i64;
    if let Some(path) = node
        .get("left_logo")
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
    {
        let logo_size = height as i64 - footer_y;
        let logo = resize_image(
            &load_image(Path::new(path))?,
            Some(logo_size as u32),
            Some(logo_size as u32),
            None,
        );
        alpha_over(&mut canvas, &logo, left_margin, footer_y);
        left_logo_width = logo_size;
    }
    if let Some(path) = node
        .get("center_logo")
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
    {
        let logo_height = value_i64(node, "center_logo_height", height as i64 - footer_y);
        let logo = resize_image(
            &load_image(Path::new(path))?,
            None,
            Some(logo_height as u32),
            None,
        );
        alpha_over(
            &mut canvas,
            &logo,
            (width as i64 - logo.width() as i64) / 2,
            footer_y + (height as i64 - footer_y - logo.height() as i64) / 2,
        );
    }
    let elem_height = (left_top.height() + left_bottom.height())
        .max(right_top.height() + right_bottom.height()) as i64
        + middle_spacing;
    let elem_margin = (bottom_margin - elem_height) / 2;
    let left_x = left_margin + left_logo_width + common_spacing;
    let right_end = width as i64 - right_margin;
    let lt_y = height as i64
        - (elem_margin + left_bottom.height() as i64 + middle_spacing + left_top.height() as i64);
    let lb_y = height as i64 - (elem_margin + left_bottom.height() as i64);
    let rt_y = lt_y + left_top.height() as i64 - right_top.height() as i64;
    let rb_y = lb_y + left_bottom.height() as i64 - right_bottom.height() as i64;
    let mut rt_x = right_end - right_top.width() as i64 - common_spacing;
    let mut rb_x = right_end - right_bottom.width() as i64 - common_spacing;
    if value_string(node, "right_alignment", "right") == "left" {
        let minimum = rt_x.min(rb_x);
        rt_x = minimum;
        rb_x = minimum;
    }
    alpha_over(&mut canvas, &left_top, left_x, lt_y);
    alpha_over(&mut canvas, &left_bottom, left_x, lb_y);
    alpha_over(&mut canvas, &right_top, rt_x, rt_y);
    alpha_over(&mut canvas, &right_bottom, rb_x, rb_y);
    if let Some(path) = node
        .get("right_logo")
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
    {
        let logo_size = elem_height;
        let delimiter_x = width as i64
            - right_margin
            - right_top.width().max(right_bottom.width()) as i64
            - common_spacing * 2
            - delimiter_width;
        let delimiter_y = (footer_y as f64 + elem_margin as f64 - logo_size as f64 * 0.05) as i64;
        let delimiter = RgbaImage::from_pixel(
            delimiter_width.max(1) as u32,
            (logo_size as f64 * 1.1) as u32,
            delimiter_color,
        );
        alpha_over(&mut canvas, &delimiter, delimiter_x, delimiter_y);
        let logo = resize_image(
            &load_image(Path::new(path))?,
            Some(logo_size as u32),
            Some(logo_size as u32),
            None,
        );
        alpha_over(
            &mut canvas,
            &logo,
            delimiter_x - common_spacing - logo_size,
            footer_y + elem_margin,
        );
    }
    Ok(canvas)
}

fn process_node(root: &Path, node: &Value, images: Vec<RgbaImage>) -> EngineResult<Vec<RgbaImage>> {
    let name = node
        .get("processor_name")
        .and_then(Value::as_str)
        .ok_or("处理器名称缺失")?;
    let result = match name {
        "solid_color" => vec![RgbaImage::from_pixel(
            value_i64(node, "width", 0).max(1) as u32,
            value_i64(node, "height", 0).max(1) as u32,
            parse_color(
                node.get("color").unwrap_or(&json!("white")),
                Rgba([255, 255, 255, 255]),
            ),
        )],
        "gradient_color" => {
            let width = value_i64(node, "width", 1).max(1) as u32;
            let height = value_i64(node, "height", 1).max(1) as u32;
            let start = parse_color(
                node.get("start_color").unwrap_or(&json!("black")),
                Rgba([0, 0, 0, 255]),
            );
            let end = parse_color(
                node.get("end_color").unwrap_or(&json!("white")),
                Rgba([255, 255, 255, 255]),
            );
            let direction = value_string(node, "direction", "horizontal");
            let method = value_string(node, "interpolate_method", "linear");
            let mut image = RgbaImage::new(width, height);
            for y in 0..height {
                for x in 0..width {
                    let raw = match direction.as_str() {
                        "vertical" => y as f64 / (height - 1).max(1) as f64,
                        "diagonal" => (x + y) as f64 / (width + height - 2).max(1) as f64,
                        "radial" => {
                            let cx = width as f64 / 2.0;
                            let cy = height as f64 / 2.0;
                            (((x as f64 - cx).powi(2) + (y as f64 - cy).powi(2)).sqrt()
                                / (cx.powi(2) + cy.powi(2)).sqrt())
                            .min(1.0)
                        }
                        _ => x as f64 / (width - 1).max(1) as f64,
                    };
                    let t = match method.as_str() {
                        "ease_in" => raw * raw,
                        "ease_out" => 1.0 - (1.0 - raw).powi(2),
                        "ease_in_out" => {
                            if raw < 0.5 {
                                2.0 * raw * raw
                            } else {
                                1.0 - (-2.0 * raw + 2.0).powi(2) / 2.0
                            }
                        }
                        _ => raw,
                    };
                    let mut pixel = [0; 4];
                    for c in 0..4 {
                        pixel[c] =
                            (start.0[c] as f64 + (end.0[c] as f64 - start.0[c] as f64) * t) as u8;
                    }
                    image.put_pixel(x, y, Rgba(pixel));
                }
            }
            vec![image]
        }
        "rich_text" => vec![generate_text(root, node)?],
        "multi_rich_text" => vec![generate_multi_text(root, node)?],
        "image" => {
            let paths = if let Some(path) = node.get("path").and_then(Value::as_str) {
                vec![path.to_owned()]
            } else {
                node.get("path")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            };
            paths
                .iter()
                .map(|path| load_image(Path::new(path)))
                .collect::<EngineResult<Vec<_>>>()?
        }
        "blur" => images
            .into_iter()
            .map(|mut image| {
                // Pillow converts every non-RGB buffer to RGB before blur.
                // Keep the corresponding opaque RGBA representation here.
                for pixel in image.pixels_mut() {
                    pixel.0[3] = 255;
                }
                gaussian_blur(&image, value_i64(node, "blur_radius", 5))
            })
            .collect(),
        "resize" => images
            .iter()
            .map(|image| {
                resize_image(
                    image,
                    node.get("width")
                        .map(|_| value_i64(node, "width", 0) as u32)
                        .filter(|v| *v > 0),
                    node.get("height")
                        .map(|_| value_i64(node, "height", 0) as u32)
                        .filter(|v| *v > 0),
                    node.get("scale").map(|_| value_f64(node, "scale", 1.0)),
                )
            })
            .collect(),
        "trim" => images
            .iter()
            .map(|image| {
                let (l, t, r, b) = foreground_bbox(
                    image,
                    value_bool(node, "trim_left", true),
                    value_bool(node, "trim_right", true),
                    value_bool(node, "trim_top", true),
                    value_bool(node, "trim_bottom", true),
                );
                imageops::crop_imm(image, l, t, r - l, b - t).to_image()
            })
            .collect(),
        "margin" => {
            let color = parse_color(
                node.get("margin_color").unwrap_or(&json!("white")),
                Rgba([255, 255, 255, 255]),
            );
            images
                .iter()
                .map(|image| {
                    add_margin(
                        image,
                        value_i64(node, "left_margin", 0),
                        value_i64(node, "right_margin", 0),
                        value_i64(node, "top_margin", 0),
                        value_i64(node, "bottom_margin", 0),
                        color,
                    )
                })
                .collect()
        }
        "margin_with_ratio" => {
            let image = images.first().ok_or("缺少输入图像")?;
            let ratio = value_string(node, "ratio", "");
            let target = ratio
                .split_once(':')
                .and_then(|(w, h)| Some(w.parse::<f64>().ok()? / h.parse::<f64>().ok()?))
                .unwrap_or(image.width() as f64 / image.height() as f64);
            let current = image.width() as f64 / image.height() as f64;
            let color = parse_color(
                node.get("margin_color").unwrap_or(&json!("white")),
                Rgba([255, 255, 255, 255]),
            );
            if current - target > 0.01 {
                let new_h = image.width() as f64 / target;
                let pad = new_h as i64 - image.height() as i64;
                vec![add_margin(image, 0, 0, pad / 2, pad - pad / 2, color)]
            } else if current - target < 0.01 {
                let new_w = image.height() as f64 * target;
                let pad = new_w as i64 - image.width() as i64;
                vec![add_margin(image, pad / 2, pad - pad / 2, 0, 0, color)]
            } else {
                images
            }
        }
        "rounded_corner" => images
            .iter()
            .map(|image| rounded_corner(image, value_i64(node, "border_radius", 10)))
            .collect(),
        "shadow" => {
            let color = parse_color(
                node.get("shadow_color").unwrap_or(&json!([0, 0, 0, 180])),
                Rgba([0, 0, 0, 180]),
            );
            images
                .iter()
                .map(|image| shadow(image, value_i64(node, "shadow_radius", 30), color))
                .collect()
        }
        "crop" => images
            .iter()
            .map(|image| {
                let width = value_i64(node, "width", image.width() as i64).max(1) as u32;
                let height = value_i64(node, "height", image.height() as i64).max(1) as u32;
                let offsets = parse_json_array(node, "offset");
                let ox = offsets.first().and_then(Value::as_i64).unwrap_or(0);
                let oy = offsets.get(1).and_then(Value::as_i64).unwrap_or(0);
                let left = ((image.width() as i64 - width as i64) / 2 + ox)
                    .clamp(0, (image.width() as i64 - width as i64).max(0));
                let top = ((image.height() as i64 - height as i64) / 2 + oy)
                    .clamp(0, (image.height() as i64 - height as i64).max(0));
                crop_with_padding(image, left, top, width, height)
            })
            .collect(),
        "concat" => vec![concat(
            &images,
            &value_string(node, "direction", "horizontal"),
            &value_string(node, "alignment", "bottom"),
            value_i64(node, "spacing", 10),
            parse_color(
                node.get("background").unwrap_or(&json!([255, 255, 255, 0])),
                Rgba([255, 255, 255, 0]),
            ),
        )],
        "alignment" => vec![alignment(images, node)],
        "watermark" => vec![watermark(
            root,
            images.first().ok_or("缺少输入图像")?,
            node,
        )?],
        "watermark_with_timestamp" => {
            let base = images.first().ok_or("缺少输入图像")?;
            let mut text_node = node.clone();
            if text_node.get("height").is_none() {
                text_node.as_object_mut().unwrap().insert(
                    "height".to_owned(),
                    json!((base.height() as f64 * 0.02) as i64),
                );
            }
            let text = generate_multi_text(root, &text_node)?;
            let mut image = base.clone();
            let x = (base.width() as f64 * 0.93) as i64 - text.width() as i64;
            let y = (base.height() as f64 * 0.95) as i64;
            alpha_over(&mut image, &text, x, y);
            vec![image]
        }
        _ => return Err(format!("未知处理器: {name}")),
    };
    Ok(result)
}

pub fn load_image(path: &Path) -> EngineResult<RgbaImage> {
    let extension = path
        .extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if matches!(extension.as_str(), "heic" | "heif") {
        #[cfg(windows)]
        return load_image_with_wic(path);
        #[cfg(not(windows))]
        return Err("当前平台没有可用的 HEIC 解码器".to_owned());
    }
    let reader = ImageReader::open(path)
        .map_err(|error| format!("无法读取图片 {}: {error}", path.display()))?
        .with_guessed_format()
        .map_err(|error| format!("无法识别图片 {}: {error}", path.display()))?;
    let mut decoder = reader.into_decoder().map_err(|error| error.to_string())?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder).map_err(|error| error.to_string())?;
    image.apply_orientation(orientation);
    Ok(image.into_rgba8())
}

#[cfg(windows)]
fn load_image_with_wic(path: &Path) -> EngineResult<RgbaImage> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::GENERIC_READ;
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICImagingFactory,
        WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnLoad,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };

    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                .map_err(|error| format!("无法初始化 Windows 图像解码器: {error}"))?;
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let decoder = factory
            .CreateDecoderFromFilename(
                PCWSTR(wide.as_ptr()),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnLoad,
            )
            .map_err(|error| format!("无法解码 HEIC {}: {error}", path.display()))?;
        let frame = decoder.GetFrame(0).map_err(|error| error.to_string())?;
        let converter = factory
            .CreateFormatConverter()
            .map_err(|error| error.to_string())?;
        converter
            .Initialize(
                &frame,
                &GUID_WICPixelFormat32bppRGBA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .map_err(|error| error.to_string())?;
        let mut width = 0;
        let mut height = 0;
        converter
            .GetSize(&mut width, &mut height)
            .map_err(|error| error.to_string())?;
        let stride = width.checked_mul(4).ok_or("HEIC 图片尺寸过大")?;
        let byte_count = stride.checked_mul(height).ok_or("HEIC 图片尺寸过大")?;
        let mut pixels = vec![0u8; byte_count as usize];
        converter
            .CopyPixels(std::ptr::null(), stride, &mut pixels)
            .map_err(|error| error.to_string())?;
        RgbaImage::from_raw(width, height, pixels).ok_or("HEIC 像素数据无效".to_owned())
    }
}

fn process_pipeline_with_source(
    root: &Path,
    nodes: &[Value],
    initial: RgbaImage,
) -> EngineResult<RgbaImage> {
    if nodes.is_empty() {
        return Err("模板没有处理节点".to_owned());
    }
    // Plan buffer uses before rendering. Move the last use instead of cloning
    // every full frame at every node, and release dead branches immediately.
    let mut uses = vec![0usize; nodes.len() + 1];
    let mut plans = Vec::with_capacity(nodes.len());
    let mut last_merger: i64 = -1;
    for (index, node) in nodes.iter().enumerate() {
        let name = node["processor_name"].as_str().unwrap_or("");
        let plan = if node.get("select").is_some() {
            let mut selected = Vec::new();
            for value in parse_json_array(node, "select") {
                let raw = value.as_i64().ok_or("select 必须是整数索引")?;
                let resolved = if raw < 0 { index as i64 + 1 + raw } else { raw };
                if resolved < 0 || resolved > index as i64 {
                    return Err(format!("无效的缓冲区索引: {raw}"));
                }
                selected.push(resolved as usize);
            }
            Some(selected)
        } else if matches!(name, "concat" | "alignment") {
            let indices = ((last_merger + 1) as usize..=index).collect();
            last_merger = index as i64;
            Some(indices)
        } else if node.get("buffer_path").is_some() {
            None
        } else {
            Some(vec![index])
        };
        if let Some(indices) = &plan {
            for &i in indices {
                uses[i] += 1;
            }
        }
        plans.push(plan);
    }
    let mut buffers = vec![Some(vec![initial])];
    for (index, (node, plan)) in nodes.iter().zip(plans).enumerate() {
        let mut input = Vec::new();
        if let Some(indices) = plan {
            for i in indices {
                uses[i] -= 1;
                let images = if uses[i] == 0 {
                    buffers[i].take().unwrap()
                } else {
                    buffers[i].as_ref().unwrap().clone()
                };
                input.extend(images);
            }
        } else {
            let paths: Vec<String> = match node.get("buffer_path") {
                Some(Value::String(path)) => vec![path.clone()],
                Some(Value::Array(paths)) => paths
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect(),
                _ => return Err("buffer_path 必须是路径或路径数组".to_owned()),
            };
            input = paths
                .iter()
                .map(|path| load_image(Path::new(path)))
                .collect::<EngineResult<Vec<_>>>()?;
        }
        let output = process_node(root, node, input)?;
        if index + 1 == nodes.len() {
            return output
                .into_iter()
                .next()
                .ok_or("处理器没有生成图像".to_owned());
        }
        buffers.push(if uses[index + 1] == 0 {
            None
        } else {
            Some(output)
        });
    }
    unreachable!()
}

pub fn process_pipeline_from_image(
    root: &Path,
    nodes: &[Value],
    initial: RgbaImage,
) -> EngineResult<RgbaImage> {
    process_pipeline_with_source(root, nodes, initial)
}

#[cfg(test)]
fn process_pipeline(root: &Path, nodes: &[Value], input_path: &Path) -> EngineResult<RgbaImage> {
    process_pipeline_from_image(root, nodes, load_image(input_path)?)
}

pub fn process_pipeline_preview_from_image(
    root: &Path,
    nodes: &[Value],
    _input_path: &Path,
    initial: RgbaImage,
    max_dimension: u32,
) -> EngineResult<RgbaImage> {
    let output = process_pipeline_with_source(root, nodes, initial)?;
    // Export drops alpha before encoding. Do so before reducing the preview,
    // otherwise premultiplied resizing changes translucent watermark edges.
    let output = DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(output).into_rgb8()).into_rgba8();
    let largest_side = output.width().max(output.height());
    if largest_side <= max_dimension {
        return Ok(output);
    }
    let scale = max_dimension as f64 / largest_side as f64;
    Ok(resize_lanczos_parallel(
        &output,
        (output.width() as f64 * scale).max(1.0) as u32,
        (output.height() as f64 * scale).max(1.0) as u32,
    ))
}

pub fn save_image(
    path: &Path,
    image: &RgbaImage,
    quality: u8,
    subsampling: u8,
) -> EngineResult<()> {
    let parent = path.parent().ok_or("输出路径无效")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    match path
        .extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => {
            let file = fs::File::create(path).map_err(|error| error.to_string())?;
            let mut encoder = jpeg_encoder::Encoder::new(BufWriter::new(file), quality);
            encoder.set_sampling_factor(match subsampling {
                2 => jpeg_encoder::SamplingFactor::F_2_2,
                1 => jpeg_encoder::SamplingFactor::F_2_1,
                _ => jpeg_encoder::SamplingFactor::F_1_1,
            });
            let rgb = DynamicImage::ImageRgba8(image.clone()).into_rgb8();
            encoder
                .encode(
                    rgb.as_raw(),
                    rgb.width() as u16,
                    rgb.height() as u16,
                    jpeg_encoder::ColorType::Rgb,
                )
                .map_err(|error| error.to_string())?;
        }
        Some("png") => {
            let file = fs::File::create(path).map_err(|error| error.to_string())?;
            // semi-utils converts the final output to RGB for every format.
            let rgb = DynamicImage::ImageRgba8(image.clone()).into_rgb8();
            image::codecs::png::PngEncoder::new(BufWriter::new(file))
                .write_image(
                    rgb.as_raw(),
                    image.width(),
                    image.height(),
                    image::ExtendedColorType::Rgb8,
                )
                .map_err(|error| error.to_string())?;
        }
        extension => return Err(format!("不支持的输出格式: {}", extension.unwrap_or(""))),
    }
    Ok(())
}

pub fn copy_runtime_resources(resources: &Path, runtime: &Path) -> EngineResult<()> {
    fs::create_dir_all(runtime).map_err(|error| error.to_string())?;
    for directory in ["config", "exiftool"] {
        let source = resources.join(directory);
        if !source.exists() {
            continue;
        }
        for entry in WalkDir::new(&source).into_iter().flatten() {
            let relative = entry
                .path()
                .strip_prefix(resources)
                .map_err(|error| error.to_string())?;
            let target = runtime.join(relative);
            if entry.path().is_dir() {
                fs::create_dir_all(&target).map_err(|error| error.to_string())?;
            } else if !target.exists() {
                fs::copy(entry.path(), target).map_err(|error| error.to_string())?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "generate fixtures with scripts/render-reference.py first"]
    fn compare_semi_utils_reference() {
        let directory =
            PathBuf::from(std::env::var_os("LITEEXIF_PARITY_DIR").expect("LITEEXIF_PARITY_DIR"));
        check_reference(&directory, true);
    }

    #[test]
    fn bundled_templates_and_text_match_semi_utils() {
        check_reference(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/render-parity"),
            false,
        );
    }

    fn check_reference(directory: &Path, write_outputs: bool) {
        let cases: Vec<Value> =
            serde_json::from_slice(&fs::read(directory.join("fixtures.json")).unwrap()).unwrap();
        let root = project_root();
        initialize_gpu_acceleration();
        let mut metrics = Vec::new();
        let mut failures = Vec::new();
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let input = directory.join(case["input"].as_str().unwrap());
            let nodes = if let Some(template) = case["template"].as_str() {
                let exif = serde_json::from_value(case["exif"].clone()).unwrap();
                let rendered = render_template(&root, template, &exif, &input, &[]).unwrap();
                serde_json::from_str::<Vec<Value>>(&rendered).unwrap()
            } else {
                serde_json::from_value(case["nodes"].clone()).unwrap()
            };
            let started = std::time::Instant::now();
            let output = process_pipeline(&root, &nodes, &input).unwrap();
            let elapsed = started.elapsed().as_secs_f64() * 1000.0;
            if write_outputs {
                output
                    .save(directory.join(format!("{name}-rust.png")))
                    .unwrap();
            }
            let expected = load_image(&directory.join(format!("{name}-reference.png"))).unwrap();
            // Full templates export RGB; isolated processors also compare alpha.
            let channels = if case["template"].is_string() { 3 } else { 4 };
            let mut error = 0u64;
            let mut maximum = 0;
            if output.dimensions() == expected.dimensions() {
                for (a, b) in output.pixels().zip(expected.pixels()) {
                    for c in 0..channels {
                        let delta = a[c].abs_diff(b[c]);
                        error += delta as u64;
                        maximum = maximum.max(delta);
                    }
                }
            }
            if output.dimensions() != expected.dimensions() || maximum > 0 {
                failures.push(format!(
                    "{name}: {:?} vs {:?}, max channel error {maximum}",
                    output.dimensions(),
                    expected.dimensions()
                ));
            }
            let mae =
                error as f64 / (output.width() as f64 * output.height() as f64 * channels as f64);
            metrics.push(json!({"name":name,"expected":expected.dimensions(),"actual":output.dimensions(),"mae":mae,"max_error":maximum,"rust_ms":elapsed,"reference_ms":case["reference_ms"]}));
        }
        if write_outputs {
            fs::write(
                directory.join("metrics.json"),
                serde_json::to_vec_pretty(&metrics).unwrap(),
            )
            .unwrap();
        }
        assert!(
            failures.is_empty(),
            "Reference mismatches:\n{}",
            failures.join("\n")
        );
    }

    fn project_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf()
    }

    fn sample_exif() -> HashMap<String, String> {
        HashMap::from([
            ("ImageWidth".to_owned(), "1248".to_owned()),
            ("ImageHeight".to_owned(), "986".to_owned()),
            ("Make".to_owned(), "NIKON CORPORATION".to_owned()),
            ("CameraModelName".to_owned(), "NIKON Z 72".to_owned()),
            ("LensModel".to_owned(), "NIKKOR Z 50mm f/1.8 S".to_owned()),
            ("FocalLengthIn35mmFormat".to_owned(), "50 mm".to_owned()),
            ("FNumber".to_owned(), "1.8".to_owned()),
            ("ShutterSpeed".to_owned(), "1/1600".to_owned()),
            ("ISO".to_owned(), "64".to_owned()),
            (
                "DateTimeOriginal".to_owned(),
                "2026-01-10 15:56:00".to_owned(),
            ),
        ])
    }

    #[test]
    fn template_dimensions_follow_oriented_pixels() {
        let mut exif = HashMap::from([
            ("ImageWidth".to_owned(), "6000".to_owned()),
            ("ImageHeight".to_owned(), "4000".to_owned()),
        ]);
        let portrait = RgbaImage::new(4000, 6000);

        normalize_exif_dimensions(&mut exif, &portrait);

        assert_eq!(exif.get("ImageWidth").map(String::as_str), Some("4000"));
        assert_eq!(exif.get("ImageHeight").map(String::as_str), Some("6000"));
    }

    #[test]
    fn upstream_exif_keys_preserve_missing_field_defaults() {
        let exif =
            parse_exif("Camera Model Name : NIKON D810\nLens : 70-200mm f/2.8\nF Number : 2.8\n");
        let rendered = render_template(&project_root(),
            "{{exif.CameraModelName}}|{{exif.LensModel|default('-')}}|{{exif.AperatureValue or exif.FNumber}}",
            &exif, Path::new("photo.jpg"), &[]).unwrap();
        assert_eq!(rendered, "NIKON D810|-|2.8");
        assert!(!exif.contains_key("LensModel"));
    }

    #[test]
    fn composition_uses_pillow_paste_semantics() {
        let mut canvas = RgbaImage::from_pixel(1, 1, Rgba([0, 100, 200, 128]));
        let source = RgbaImage::from_pixel(1, 1, Rgba([200, 30, 0, 64]));

        alpha_over(&mut canvas, &source, 0, 0);

        // Equivalent to Pillow: dst.paste(src, (0, 0), src)
        assert_eq!(canvas.get_pixel(0, 0).0, [50, 82, 150, 112]);
    }

    #[test]
    fn crop_preserves_requested_size_outside_source() {
        let mut source = RgbaImage::new(2, 2);
        source.put_pixel(1, 1, Rgba([10, 20, 30, 255]));

        let output = crop_with_padding(&source, 0, 0, 3, 3);

        assert_eq!(output.dimensions(), (3, 3));
        assert_eq!(output.get_pixel(1, 1).0, [10, 20, 30, 255]);
        assert_eq!(output.get_pixel(2, 2).0, [0, 0, 0, 0]);
    }

    #[test]
    fn gaussian_blur_preserves_a_uniform_image() {
        let source = RgbaImage::from_pixel(32, 48, Rgba([17, 91, 203, 255]));
        let output = gaussian_blur(&source, 20);

        assert_eq!(output.dimensions(), source.dimensions());
        assert!(output.pixels().all(|pixel| pixel.0 == [17, 91, 203, 255]));
    }

    #[test]
    fn gpu_blur_matches_cpu_byte_for_byte() {
        let mut source = RgbaImage::new(37, 23);
        for (x, y, pixel) in source.enumerate_pixels_mut() {
            *pixel = Rgba([
                ((x * 17 + y * 29) % 256) as u8,
                ((x * x + y * 11) % 256) as u8,
                ((x * 7 + y * y) % 256) as u8,
                ((x * 13 + y * 19 + 31) % 256) as u8,
            ]);
        }

        for radius in [1, 3, 9] {
            let cpu = gaussian_blur_cpu(&source, radius);
            let (gpu, adapter) = crate::gpu::blur(&source, radius)
                .unwrap_or_else(|error| panic!("GPU test could not run: {error}"));
            assert_eq!(
                gpu.as_raw(),
                cpu.as_raw(),
                "GPU mismatch on {adapter}, radius {radius}"
            );
        }
    }

    #[test]
    fn preview_and_export_share_validated_gpu_blur() {
        let root = project_root();
        let source_path =
            std::env::temp_dir().join(format!("liteexif-gpu-pipeline-{}.png", std::process::id()));
        let mut source = RgbaImage::new(800, 1200);
        for (x, y, pixel) in source.enumerate_pixels_mut() {
            *pixel = Rgba([
                ((x * 3 + y * 5) % 256) as u8,
                ((x * 7 + y * 11) % 256) as u8,
                ((x * 13 + y * 17) % 256) as u8,
                255,
            ]);
        }
        save_image(&source_path, &source, 100, 0).unwrap();
        let nodes = vec![json!({"processor_name": "blur", "blur_radius": 7})];

        let preview =
            process_pipeline_preview_from_image(&root, &nodes, &source_path, source.clone(), 512)
                .unwrap();
        assert_eq!(preview.height(), 512);
        assert_eq!(gpu_acceleration_status().0, "validated");

        let export = process_pipeline(&root, &nodes, &source_path).unwrap();
        assert_eq!(export.dimensions(), source.dimensions());
        assert_eq!(gpu_acceleration_status().0, "validated");
        fs::remove_file(source_path).unwrap();
    }

    #[test]
    #[ignore = "manual full-frame GPU performance check"]
    fn gpu_full_frame_performance() {
        let source = RgbaImage::from_pixel(7360, 4912, Rgba([37, 91, 173, 255]));
        let started = std::time::Instant::now();
        let (_, adapter) = crate::gpu::blur(&source, 147).unwrap();
        eprintln!("full-frame GPU blur on {adapter}: {:?}", started.elapsed());
    }

    #[test]
    #[ignore = "set LITEEXIF_BENCH_IMAGE for a local full-pipeline measurement"]
    fn actual_background_pipeline_performance() {
        let source = std::env::var_os("LITEEXIF_BENCH_IMAGE")
            .map(PathBuf::from)
            .expect("LITEEXIF_BENCH_IMAGE is required");
        let root = project_root();
        let template = fs::read_to_string(root.join("config/templates/背景模糊.json")).unwrap();
        let exif = get_exif(&root, &source);
        let rendered = render_template(&root, &template, &exif, &source, &[]).unwrap();
        let nodes: Vec<Value> = serde_json::from_str(&rendered).unwrap();
        let started = std::time::Instant::now();
        let output = process_pipeline(&root, &nodes, &source).unwrap();
        eprintln!(
            "actual background pipeline: {:?}, output {}x{}",
            started.elapsed(),
            output.width(),
            output.height()
        );
    }

    #[test]
    fn font_paths_accept_python_template_prefix() {
        let font = load_font(&project_root(), Some("fonts/AlibabaPuHuiTi-2-85-Bold.otf"));
        assert!(font.is_ok());
    }

    #[test]
    fn every_bundled_template_renders_to_json() {
        let root = project_root();
        for name in list_templates(&root) {
            let source =
                fs::read_to_string(root.join("config/templates").join(format!("{name}.json")))
                    .unwrap();
            let rendered = render_template(
                &root,
                &source,
                &sample_exif(),
                &root.join("static/standard2.jpeg"),
                &[],
            )
            .unwrap_or_else(|error| panic!("template {name}: {error}"));
            serde_json::from_str::<Vec<Value>>(&rendered)
                .unwrap_or_else(|error| panic!("template {name}: {error}\n{rendered}"));
        }
    }

    #[test]
    fn standard_template_produces_an_image() {
        let root = project_root();
        let source = fs::read_to_string(root.join("config/templates/标准水印2.json")).unwrap();
        let input = root.join("static/standard2.jpeg");
        let rendered = render_template(&root, &source, &sample_exif(), &input, &[]).unwrap();
        let nodes: Vec<Value> = serde_json::from_str(&rendered).unwrap();
        let output = process_pipeline(&root, &nodes, &input).unwrap();
        assert!(output.width() >= 1248);
        assert!(output.height() >= 986);
    }

    #[test]
    fn buffer_path_reloads_the_requested_image() {
        let root = project_root();
        let input = root.join("static/normal1.jpeg");
        let replacement = root.join("config/logos/default.png");
        let expected = load_image(&replacement).unwrap();
        let nodes = vec![
            json!({"processor_name":"resize", "scale":0.5}),
            json!({
                "processor_name":"blur",
                "blur_radius":1,
                "buffer_path":[replacement.to_string_lossy()]
            }),
        ];

        let output = process_pipeline(&root, &nodes, &input).unwrap();
        assert_eq!(output.dimensions(), expected.dimensions());
    }

    #[test]
    fn nikon_template_preserves_expected_output_geometry() {
        let root = project_root();
        let input = root.join("static/normal1.jpeg");
        let mut exif = sample_exif();
        exif.insert("ImageWidth".to_owned(), "1280".to_owned());
        exif.insert("ImageHeight".to_owned(), "853".to_owned());
        let source =
            fs::read_to_string(root.join("config/templates/尼康专用背景模糊.json")).unwrap();
        let rendered = render_template(&root, &source, &exif, &input, &[]).unwrap();
        let nodes: Vec<Value> = serde_json::from_str(&rendered).unwrap();

        let output = process_pipeline(&root, &nodes, &input).unwrap();
        assert_eq!(output.dimensions(), (1728, 1151));
    }

    #[test]
    fn list_directory_pages_and_stays_shallow() {
        let base = std::env::temp_dir().join(format!("liteexif-list-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let nested = base.join("album").join("day1");
        fs::create_dir_all(&nested).unwrap();
        for index in 0..5 {
            fs::write(base.join(format!("top{index}.jpg")), b"x").unwrap();
        }
        fs::write(nested.join("inner.jpg"), b"x").unwrap();
        fs::write(base.join("notes.txt"), b"x").unwrap();

        let suffixes = vec![".jpg".to_owned()];

        // One page is capped, and directories are returned without children so
        // the frontend decides when to read deeper levels.
        let (first, has_more) = list_directory(&base, &suffixes, 0, 3);
        assert_eq!(first.len(), 3);
        assert!(has_more);
        assert!(first.iter().all(|node| node.children.is_none()));

        let (second, has_more) = list_directory(&base, &suffixes, 3, 3);
        assert!(!has_more);
        // One sub-directory plus five supported files; notes.txt is filtered.
        assert_eq!(first.len() + second.len(), 6);

        // The nested image only appears once that folder is requested.
        let (album, _) = list_directory(&base.join("album"), &suffixes, 0, 10);
        assert!(album.iter().any(|node| node.label == "day1"));
        assert!(!album.iter().any(|node| node.label == "inner.jpg"));

        let _ = fs::remove_dir_all(&base);
    }
}
