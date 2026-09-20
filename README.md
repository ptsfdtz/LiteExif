# LiteExif

LiteExif 是使用 Tauri 2、React 和 Rust 实现的本地照片批处理工具。它读取照片 EXIF，并通过 JSON 模板生成相机参数水印、品牌 Logo、圆角、阴影、留白和模糊背景。

## 开发

需要 Node.js 22、Rust stable 和 Windows WebView2。

```powershell
npm install
./scripts/fetch-exiftool.ps1
npm run tauri dev
```

## 构建

```powershell
npm run desktop:build
```

安装包输出到 `src-tauri/target/release/bundle/nsis/`。

## 技术结构

- `src/`：React 与 TypeScript 桌面界面
- `src-tauri/src/`：Rust 配置、文件、EXIF、模板和图像处理引擎
- `config/templates/`：水印处理模板
- `config/fonts/`：水印字体
- `config/logos/`：相机品牌 Logo

图像处理由 Rust 引擎完成。ExifTool 仅用于读取元数据，其发布基于 GPL v1 与 Artistic License 2.0。

渲染规则对齐 semi-utils：使用原生 FreeType 绘制文字，按 Pillow 的规则处理字体度量、裁边、透明图缩放、圆角和模糊。应用和安装包不需要 Python。FreeType 在 Windows 构建时静态编译。

EXIF 缺失字段使用原项目的模板默认值，不自动借用其他字段补全。对照范围与本机性能结果见 [渲染对照记录](docs/render-parity.md)。

批处理逐张调度以限制内存，图像处理内部使用 Rayon 并行；模糊支持经过 CPU 像素比对验证的 DX12 加速。预览先完成与导出相同的全分辨率排版，再缩小到最长边 1600 像素；第一次预览可能比原来的低分辨率预览慢，重复预览使用缓存。

## 验证

```powershell
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

测试包含七套模板的横竖版参考图，以及文字、透明缩放、圆角、阴影和模糊的逐像素检查。参考环境是 Windows Pillow 12.1.0（BASIC 字体排版）；JPEG 编码字节和不同 HEIC 解码器不保证一致。

仅开发时重新生成 semi-utils 对照图需要 Python：

```powershell
python -m venv .venv-renderer
.\.venv-renderer\Scripts\python.exe -m pip install -r scripts/reference-requirements.txt
.\.venv-renderer\Scripts\python.exe scripts/render-reference.py C:/path/to/semi-utils
$env:LITEEXIF_PARITY_DIR = (Resolve-Path tmp/render-parity).Path
cargo test --manifest-path src-tauri/Cargo.toml --lib compare_semi_utils_reference -- --ignored
```

对照结果与耗时写入 `tmp/render-parity/metrics.json`。加 `--benchmark` 可生成 600 万像素的对照组，输出到 `tmp/render-benchmark`；`--update-golden` 用于明确更新仓库内的参考图。

## 许可证

项目基于 [Apache License 2.0](LICENSE) 发布。

渲染算法和字体依赖的来源与许可证见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
