mod engine;
mod gpu;

use engine::{
    copy_runtime_resources, get_exif, gpu_acceleration_status, initialize_gpu_acceleration,
    list_files as scan_files, list_templates, load_image, process_pipeline,
    process_pipeline_preview_from_image, render_template, save_image, IniConfig,
};
use rayon::prelude::*;
use serde_json::{json, Value};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use tauri::{AppHandle, Emitter, Manager};

fn runtime_root(app: &AppHandle) -> Result<PathBuf, String> {
    if cfg!(debug_assertions) {
        return PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(Path::to_path_buf)
            .ok_or("无法定位项目目录".to_owned());
    }
    let resources = app
        .path()
        .resource_dir()
        .map_err(|error| error.to_string())?;
    let runtime = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("runtime-rust");
    copy_runtime_resources(&resources, &runtime)?;
    Ok(runtime)
}

fn load_config(root: &Path) -> Result<IniConfig, String> {
    IniConfig::load(&root.join("config/config.ini"))
}

#[tauri::command]
fn get_config(app: AppHandle) -> Result<Value, String> {
    let root = runtime_root(&app)?;
    let config = load_config(&root)?;
    let template_name = config.get("render", "template_name")?;
    let template = fs::read_to_string(
        root.join("config/templates")
            .join(format!("{template_name}.json")),
    )
    .map_err(|error| error.to_string())?;
    Ok(json!({
        "input_folder": config.get("DEFAULT", "input_folder")?,
        "output_folder": config.get("DEFAULT", "output_folder")?,
        "override_existed": config.get_bool("DEFAULT", "override_existed")?,
        "template_name": template_name,
        "template": template,
        "quality": config.get_i64("DEFAULT", "quality")?,
        "templates": list_templates(&root),
    }))
}

#[tauri::command]
fn save_config(app: AppHandle, config: Value) -> Result<Value, String> {
    let root = runtime_root(&app)?;
    let mut current = load_config(&root)?;
    for key in ["input_folder", "output_folder", "quality"] {
        if let Some(value) = config.get(key) {
            current.set(
                "DEFAULT",
                key,
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string()),
            );
        }
    }
    if let Some(value) = config.get("override_existed").and_then(Value::as_bool) {
        current.set("DEFAULT", "override_existed", value);
    }
    if let Some(value) = config.get("template_name").and_then(Value::as_str) {
        current.set("render", "template_name", value);
    }
    current.save(&root.join("config/config.ini"))?;
    if let (Some(name), Some(content)) = (
        config.get("template_name").and_then(Value::as_str),
        config.get("template").and_then(Value::as_str),
    ) {
        serde_json::from_str::<Value>(content)
            .map_err(|error| format!("JSON 格式错误: {error}"))?;
        fs::write(
            root.join("config/templates").join(format!("{name}.json")),
            content,
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(json!({"message":"配置已保存"}))
}

#[tauri::command]
fn list_files(app: AppHandle) -> Result<Value, String> {
    let root = runtime_root(&app)?;
    let config = load_config(&root)?;
    let suffixes: Vec<String> = config
        .get("DEFAULT", "supported_file_suffixes")?
        .split(',')
        .map(str::to_owned)
        .collect();
    let input = PathBuf::from(config.get("DEFAULT", "input_folder")?);
    let output = PathBuf::from(config.get("DEFAULT", "output_folder")?);
    Ok(json!({
        "input_files": [{"children": scan_files(&input, &suffixes), "label":"Root"}],
        "output_files": [{"children": scan_files(&output, &suffixes), "label":"Root"}],
    }))
}

#[tauri::command]
fn get_acceleration_status() -> Value {
    let (state, adapter) = gpu_acceleration_status();
    json!({
        "backend": if state == "validated" { "GPU (DX12 compute)" } else { "CPU" },
        "state": state,
        "adapter": adapter,
        "pixel_exact": state == "validated",
        "scope": "preview-and-export",
    })
}

#[tauri::command]
fn get_template(app: AppHandle, template_name: String) -> Result<Value, String> {
    let root = runtime_root(&app)?;
    let content = fs::read_to_string(
        root.join("config/templates")
            .join(format!("{template_name}.json")),
    )
    .map_err(|error| error.to_string())?;
    Ok(json!({"template_name":template_name,"content":content}))
}

#[tauri::command]
fn create_template(
    app: AppHandle,
    template_name: String,
    content: String,
) -> Result<Value, String> {
    let root = runtime_root(&app)?;
    let name = template_name.trim();
    if name.is_empty() || name.contains(['/', '\\']) {
        return Err("模板名称无效".to_owned());
    }
    serde_json::from_str::<Value>(&content).map_err(|error| format!("JSON 格式错误: {error}"))?;
    let path = root.join("config/templates").join(format!("{name}.json"));
    if path.exists() {
        return Err(format!("模板已存在: {name}"));
    }
    fs::write(path, content).map_err(|error| error.to_string())?;
    Ok(json!({"message":format!("已创建模板: {name}")}))
}

#[tauri::command]
fn prepare_preview(app: AppHandle, path: String) -> Result<Value, String> {
    let source = PathBuf::from(path);
    if !source.is_file() {
        return Err("图片不存在".to_owned());
    }
    if matches!(
        source
            .extension()
            .map(|value| value.to_string_lossy().to_ascii_lowercase())
            .as_deref(),
        Some("heic") | Some("heif")
    ) {
        let image = load_image(&source)?;
        let cache = app
            .path()
            .app_cache_dir()
            .map_err(|error| error.to_string())?
            .join("preview");
        fs::create_dir_all(&cache).map_err(|error| error.to_string())?;
        let modified = source
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|value| value.as_nanos())
            .unwrap_or(0);
        let target = cache.join(format!(
            "{}-{modified}.png",
            source.file_stem().unwrap_or_default().to_string_lossy()
        ));
        if !target.exists() {
            save_image(&target, &image, 100, 0)?;
        }
        return Ok(json!({"path":target}));
    }
    Ok(json!({"path":source}))
}

#[tauri::command]
async fn prepare_processed_preview(
    app: AppHandle,
    path: String,
    template: String,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = runtime_root(&app)?;
        let source = PathBuf::from(path);
        if !source.is_file() {
            return Err("图片不存在".to_owned());
        }

        let mut exif = get_exif(&root, &source);
        let original = load_image(&source)?;
        let largest_side = original.width().max(original.height());
        // A preview should be quick and fit the preview surface.  Rendering a
        // 1600px source with a large blur radius can consume gigabytes before
        // the user sees anything, especially for portrait photographs.
        const PREVIEW_MAX_DIMENSION: u32 = 512;
        let scale = if largest_side > PREVIEW_MAX_DIMENSION {
            PREVIEW_MAX_DIMENSION as f64 / largest_side as f64
        } else {
            1.0
        };
        // The Python implementation renders vw/vh from ExifTool's original
        // dimensions, then applies EXIF orientation while decoding pixels.
        // Do not replace them with the oriented dimensions here: that swaps
        // template geometry for portrait photos carrying an orientation tag.
        let exif_dimension = |key: &str, fallback: u32| {
            exif.get(key)
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(fallback)
        };
        let preview_exif_width =
            (exif_dimension("ImageWidth", original.width()) as f64 * scale) as u32;
        let preview_exif_height =
            (exif_dimension("ImageHeight", original.height()) as f64 * scale) as u32;
        exif.insert("ImageWidth".to_owned(), preview_exif_width.to_string());
        exif.insert("ImageHeight".to_owned(), preview_exif_height.to_string());
        let rendered = render_template(&root, &template, &exif, &source, &[])?;
        let nodes: Vec<Value> = serde_json::from_str(&rendered)
            .map_err(|error| format!("模板渲染结果无效: {error}"))?;

        let modified = source
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|value| value.as_nanos())
            .unwrap_or(0);
        let mut hasher = DefaultHasher::new();
        // Bump whenever rendering semantics change.  Without this, a cached
        // preview made before EXIF normalization keeps showing stale fields.
        const PREVIEW_RENDER_VERSION: u8 = 4;
        PREVIEW_RENDER_VERSION.hash(&mut hasher);
        source.hash(&mut hasher);
        modified.hash(&mut hasher);
        template.hash(&mut hasher);
        let cache = app
            .path()
            .app_cache_dir()
            .map_err(|error| error.to_string())?
            .join("processed-preview");
        fs::create_dir_all(&cache).map_err(|error| error.to_string())?;
        let target = cache.join(format!("{:016x}.jpg", hasher.finish()));
        let cache_hit = target.exists();
        if !cache_hit {
            let preview = process_pipeline_preview_from_image(
                &root,
                &nodes,
                &source,
                original,
                PREVIEW_MAX_DIMENSION,
            )?;
            save_image(&target, &preview, 75, 2)?;
        }
        let (gpu_state, adapter) = gpu_acceleration_status();
        Ok(json!({
            "path": target,
            "cache_hit": cache_hit,
            "acceleration": {
                "backend": if gpu_state == "validated" { "GPU (DX12 compute)" } else { "CPU" },
                "state": gpu_state,
                "adapter": adapter,
                "pixel_exact": gpu_state == "validated",
                "scope": "preview-and-export",
            }
        }))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn start_processing(app: AppHandle, selected_items: Vec<String>) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = runtime_root(&app)?;
        let config = load_config(&root)?;
        let input_root = PathBuf::from(config.get("DEFAULT", "input_folder")?);
        let output_root = PathBuf::from(config.get("DEFAULT", "output_folder")?);
        let overwrite = config.get_bool("DEFAULT", "override_existed")?;
        let quality = config.get_i64("DEFAULT", "quality")?.clamp(1, 100) as u8;
        let subsampling = config.get_i64("DEFAULT", "subsampling")? as u8;
        let template_name = config.get("render", "template_name")?;
        let template_source = fs::read_to_string(root.join("config/templates").join(format!("{template_name}.json"))).map_err(|error| error.to_string())?;
        let total = selected_items.len();
        let processed = AtomicUsize::new(0); let success = AtomicUsize::new(0); let failure = AtomicUsize::new(0); let skipped = AtomicUsize::new(0);
        app.emit("processing-progress", json!({"event":"start","data":{"total":total,"processed":0,"success":0,"failure":0,"skipped":0,"percent":0,"message":format!("开始处理 {total} 个文件")}})).map_err(|error| error.to_string())?;

        // Full-resolution blur templates retain multiple frame-sized buffers.
        // Keep batch parallelism bounded so several camera originals cannot
        // exhaust memory before the first export finishes.
        // A background-blur pipeline can retain several full-resolution and
        // 2x-sized buffers. Running two D810 files concurrently pushes the
        // process above 4 GB and causes paging, which is slower than serial
        // GPU work on a single shared device.
        let worker_count = 1;
        rayon::ThreadPoolBuilder::new()
            .num_threads(worker_count)
            .build()
            .map_err(|error| error.to_string())?
            .install(|| selected_items.par_iter().for_each(|item| {
            let source = PathBuf::from(item);
            let name = source.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let relative = source.strip_prefix(&input_root).unwrap_or(&source);
            let target = output_root.join(relative);
            let _ = app.emit("processing-progress", json!({"event":"progress","data":{"total":total,"processed":processed.load(Ordering::Relaxed),"success":success.load(Ordering::Relaxed),"failure":failure.load(Ordering::Relaxed),"skipped":skipped.load(Ordering::Relaxed),"current":name,"percent":if total==0{100}else{processed.load(Ordering::Relaxed)*100/total},"message":format!("正在处理: {name}")}}));
            let status = if target.exists() && !overwrite { skipped.fetch_add(1, Ordering::Relaxed); "skipped" }
            else {
                let result = (|| -> Result<(), String> {
                    let exif = get_exif(&root, &source);
                    let rendered = render_template(&root, &template_source, &exif, &source, &selected_items)?;
                    let nodes: Vec<Value> = serde_json::from_str(&rendered).map_err(|error| format!("模板渲染结果无效: {error}"))?;
                    let output = process_pipeline(&root, &nodes, &source)?;
                    save_image(&target, &output, quality, subsampling)
                })();
                if result.is_ok() { success.fetch_add(1, Ordering::Relaxed); "success" } else { failure.fetch_add(1, Ordering::Relaxed); "failure" }
            };
            let done = processed.fetch_add(1, Ordering::Relaxed) + 1;
            let labels = match status { "success"=>"完成", "skipped"=>"跳过", _=>"失败" };
            let _ = app.emit("processing-progress", json!({"event":"progress","data":{"total":total,"processed":done,"success":success.load(Ordering::Relaxed),"failure":failure.load(Ordering::Relaxed),"skipped":skipped.load(Ordering::Relaxed),"current":name,"percent":if total==0{100}else{done*100/total},"message":format!("{labels}: {name}")}}));
        }));
        let result = json!({"total":total,"processed":processed.load(Ordering::Relaxed),"success":success.load(Ordering::Relaxed),"failure":failure.load(Ordering::Relaxed),"skipped":skipped.load(Ordering::Relaxed),"percent":100,"message":format!("处理完成：成功 {}，跳过 {}，失败 {}",success.load(Ordering::Relaxed),skipped.load(Ordering::Relaxed),failure.load(Ordering::Relaxed))});
        app.emit("processing-progress", json!({"event":"complete","data":result})).map_err(|error| error.to_string())?;
        Ok(result)
    }).await.map_err(|error| error.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Validate the compute backend on a tiny deterministic image before any
    // user preview/export so real photos never pay for CPU+GPU double work.
    initialize_gpu_acceleration();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_config,
            get_acceleration_status,
            save_config,
            list_files,
            get_template,
            create_template,
            prepare_preview,
            prepare_processed_preview,
            start_processing,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run LiteExif");
}
