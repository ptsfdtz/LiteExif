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

## 许可证

项目基于 [Apache License 2.0](LICENSE) 发布。
