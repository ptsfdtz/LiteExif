use ab_glyph::{Font, FontArc, PxScale, ScaleFont};
use image::imageops::{self, FilterType};
use image::{DynamicImage, ImageDecoder, ImageEncoder, ImageReader, Rgba, RgbaImage};
use imageproc::drawing::{draw_text_mut, text_size};
use minijinja::{context, Environment, Error as TemplateError, State, Value as TemplateValue};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::process::Command;
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

pub fn list_files(path: &Path, suffixes: &[String]) -> Vec<FileNode> {
    if !path.exists() {
        return Vec::new();
    }
    let mut directories = Vec::new();
    let mut files = Vec::new();
    let Ok(entries) = fs::read_dir(path) else {
        return Vec::new();
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
        let a_time = fs::metadata(&a.1).and_then(|meta| meta.modified()).ok();
        let b_time = fs::metadata(&b.1).and_then(|meta| meta.modified()).ok();
        b_time
            .cmp(&a_time)
            .then_with(|| b.0.to_lowercase().cmp(&a.0.to_lowercase()))
    });
    let mut result = Vec::new();
    for (name, child_path) in directories {
        let children = list_files(&child_path, suffixes);
        if !children.is_empty() {
            result.push(FileNode {
                label: name,
                value: child_path.to_string_lossy().into_owned(),
                is_file: None,
                children: Some(children),
            });
        }
    }
    for (name, file_path) in files {
        let suffix = file_path
            .extension()
            .map(|value| format!(".{}", value.to_string_lossy().to_ascii_lowercase()))
            .unwrap_or_default();
        if suffixes.iter().any(|value| value == &suffix) {
            result.push(FileNode {
                label: name,
                value: file_path.to_string_lossy().into_owned(),
                is_file: Some(true),
                children: None,
            });
        }
    }
    result
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
    let mut result = HashMap::new();
    let Ok(output) = Command::new(executable)
        .args(["-d", "%Y-%m-%d %H:%M:%S%3f%z"])
        .arg(path)
        .output()
    else {
        return result;
    };
    let text = String::from_utf8_lossy(&output.stdout);
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
            let source_alpha = source[3] as f32 / 255.0;
            let inverse = 1.0 - source_alpha;
            let out_alpha = source_alpha + target[3] as f32 / 255.0 * inverse;
            let mut output = [0u8; 4];
            if out_alpha > 0.0 {
                for channel in 0..3 {
                    output[channel] = ((source[channel] as f32 * source_alpha
                        + target[channel] as f32 * target[3] as f32 / 255.0 * inverse)
                        / out_alpha)
                        .round() as u8;
                }
            }
            output[3] = (out_alpha * 255.0).round() as u8;
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
        (Some(w), None, _) => (
            w,
            (image.height() as f64 * w as f64 / image.width() as f64) as u32,
        ),
        (None, Some(h), _) => (
            (image.width() as f64 * h as f64 / image.height() as f64) as u32,
            h,
        ),
        (_, _, Some(factor)) => (
            (image.width() as f64 * factor) as u32,
            (image.height() as f64 * factor) as u32,
        ),
        _ => image.dimensions(),
    };
    imageops::resize(
        image,
        target_width.max(1),
        target_height.max(1),
        FilterType::Lanczos3,
    )
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

fn load_font(root: &Path, requested: Option<&str>) -> EngineResult<FontArc> {
    let requested_path = requested.map(PathBuf::from).map(|path| {
        if path.is_absolute() {
            path
        } else {
            root.join("config/fonts").join(path)
        }
    });
    let candidates = [
        requested_path,
        Some(root.join("config/fonts/AlibabaPuHuiTi-2-45-Light.otf")),
        Some(root.join("config/fonts/Roboto-Regular.ttf")),
    ];
    for path in candidates.into_iter().flatten() {
        if let Ok(bytes) = fs::read(path) {
            if let Ok(font) = FontArc::try_from_vec(bytes) {
                return Ok(font);
            }
        }
    }
    Err("没有可用字体".to_owned())
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
    let scale = 512.0f32;
    let (width, _) = text_size(scale, &font, &text);
    let scaled_font = font.as_scaled(PxScale::from(scale));
    let height = (scaled_font.ascent() - scaled_font.descent()).ceil().max(1.0) as u32;
    let mut image = RgbaImage::from_pixel(width.max(1), height.max(1), Rgba([0, 0, 0, 0]));
    draw_text_mut(&mut image, color, 0, 0, scale, &font, &text);
    let trim = value_bool(node, "trim", false);
    if trim && image.width() > 0 && image.height() > 0 {
        let (left, top, right, bottom) = foreground_bbox(&image, true, true, true, true);
        image = imageops::crop_imm(&image, left, top, right - left, bottom - top).to_image();
    }
    let requested_height = value_i64(node, "height", 100) as f64;
    let target_height = if value_bool(node, "is_bold", false) {
        requested_height * 1.13
    } else {
        requested_height
    };
    Ok(resize_image(
        &image,
        None,
        Some(target_height.max(1.0) as u32),
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
    alpha_over(&mut canvas, image, left, top);
    canvas
}

fn rounded_corner(image: &RgbaImage, radius: i64) -> RgbaImage {
    let mut output = image.clone();
    let radius = radius.max(0) as f64;
    if radius == 0.0 {
        return output;
    }
    for y in 0..output.height() {
        for x in 0..output.width() {
            let nearest_x = if (x as f64) < radius {
                radius
            } else if x as f64 > output.width() as f64 - radius {
                output.width() as f64 - radius
            } else {
                x as f64
            };
            let nearest_y = if (y as f64) < radius {
                radius
            } else if y as f64 > output.height() as f64 - radius {
                output.height() as f64 - radius
            } else {
                y as f64
            };
            if ((x as f64 - nearest_x).powi(2) + (y as f64 - nearest_y).powi(2)).sqrt() > radius {
                output.get_pixel_mut(x, y).0[3] = 0;
            }
        }
    }
    output
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
        shadow_pixel.0[3] = ((color.0[3] as u16 * pixel.0[3] as u16) / 255) as u8;
        layer.put_pixel(x + padding as u32, y + padding as u32, shadow_pixel);
    }
    let mut layer = imageops::blur(&layer, radius as f32);
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
        let delimiter_y = footer_y + elem_margin - (logo_size as f64 * 0.05) as i64;
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
            .map(|image| imageops::blur(&image, value_i64(node, "blur_radius", 5) as f32))
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
                let width = value_i64(node, "width", image.width() as i64)
                    .min(image.width() as i64)
                    .max(1) as u32;
                let height = value_i64(node, "height", image.height() as i64)
                    .min(image.height() as i64)
                    .max(1) as u32;
                let offsets = parse_json_array(node, "offset");
                let ox = offsets.first().and_then(Value::as_i64).unwrap_or(0);
                let oy = offsets.get(1).and_then(Value::as_i64).unwrap_or(0);
                let left = ((image.width() - width) as i64 / 2 + ox)
                    .clamp(0, (image.width() - width) as i64) as u32;
                let top = ((image.height() - height) as i64 / 2 + oy)
                    .clamp(0, (image.height() - height) as i64) as u32;
                imageops::crop_imm(image, left, top, width, height).to_image()
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

fn same_file_path(left: &Path, right: &Path) -> bool {
    left == right
        || left
            .canonicalize()
            .ok()
            .zip(right.canonicalize().ok())
            .is_some_and(|(left, right)| left == right)
}

fn process_pipeline_with_source(
    root: &Path,
    nodes: &[Value],
    input_path: &Path,
    initial: RgbaImage,
    source_override: Option<&RgbaImage>,
) -> EngineResult<RgbaImage> {
    if nodes.is_empty() {
        return Err("模板没有处理节点".to_owned());
    }
    let mut output = vec![initial.clone()];
    let mut all_buffers = vec![vec![initial]];
    let mut last_merger: i64 = -1;
    for (index, node) in nodes.iter().enumerate() {
        let name = node
            .get("processor_name")
            .and_then(Value::as_str)
            .unwrap_or("");
        let input = if node.get("buffer_path").is_some()
            && node.get("select").is_none()
            && !matches!(name, "concat" | "alignment")
        {
            let paths: Vec<String> = match node.get("buffer_path") {
                Some(Value::String(path)) => vec![path.clone()],
                Some(Value::Array(paths)) => paths
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect(),
                _ => Vec::new(),
            };
            paths
                .iter()
                .map(|path| {
                    if let Some(source) = source_override.filter(|_| same_file_path(Path::new(path), input_path)) {
                        Ok(source.clone())
                    } else {
                        load_image(Path::new(path))
                    }
                })
                .collect::<EngineResult<Vec<_>>>()?
        } else if node.get("select").is_some() {
            parse_json_array(node, "select")
                .iter()
                .filter_map(Value::as_i64)
                .filter_map(|idx| all_buffers.get(idx as usize))
                .flatten()
                .cloned()
                .collect()
        } else if matches!(name, "concat" | "alignment") {
            let start = (last_merger + 1) as usize;
            let merged = all_buffers[start..=index]
                .iter()
                .flatten()
                .cloned()
                .collect();
            last_merger = index as i64;
            merged
        } else {
            output
        };
        output = process_node(root, node, input)?;
        all_buffers.push(output.clone());
    }
    output
        .into_iter()
        .next()
        .ok_or("处理器没有生成图像".to_owned())
}

pub fn process_pipeline(
    root: &Path,
    nodes: &[Value],
    input_path: &Path,
) -> EngineResult<RgbaImage> {
    let initial = load_image(input_path)?;
    process_pipeline_with_source(root, nodes, input_path, initial, None)
}

pub fn process_pipeline_preview(
    root: &Path,
    nodes: &[Value],
    input_path: &Path,
    max_dimension: u32,
) -> EngineResult<RgbaImage> {
    let initial = load_image(input_path)?;
    let largest_side = initial.width().max(initial.height());
    let preview_source = if largest_side > max_dimension {
        let scale = max_dimension as f64 / largest_side as f64;
        resize_image(&initial, None, None, Some(scale))
    } else {
        initial
    };
    process_pipeline_with_source(
        root,
        nodes,
        input_path,
        preview_source.clone(),
        Some(&preview_source),
    )
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
            encoder.set_sampling_factor(if subsampling == 2 {
                jpeg_encoder::SamplingFactor::F_2_2
            } else {
                jpeg_encoder::SamplingFactor::F_1_1
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
            image::codecs::png::PngEncoder::new(BufWriter::new(file))
                .write_image(
                    image.as_raw(),
                    image.width(),
                    image.height(),
                    image::ExtendedColorType::Rgba8,
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
        let source = fs::read_to_string(root.join("config/templates/尼康专用背景模糊.json")).unwrap();
        let rendered = render_template(&root, &source, &exif, &input, &[]).unwrap();
        let nodes: Vec<Value> = serde_json::from_str(&rendered).unwrap();

        let output = process_pipeline(&root, &nodes, &input).unwrap();
        assert_eq!(output.dimensions(), (1728, 1151));
    }
}
