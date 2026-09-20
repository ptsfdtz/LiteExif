<div align="center">

<img src="public/logo.svg" width="88" height="88" alt="LiteExif logo" />

# LiteExif

**本地优先的照片批处理与水印工具** —— 读取照片 EXIF，通过 JSON 模板生成相机参数水印、品牌 Logo、圆角、阴影与模糊背景。

[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.1.0-success.svg)](https://github.com/ptsfdtz/LiteExif/releases)
[![Platform](https://img.shields.io/badge/platform-Windows-0078D6.svg)](#系统要求)
[![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB.svg)](https://tauri.app)
[![React](https://img.shields.io/badge/React-19-61DAFB.svg)](https://react.dev)
[![Rust](https://img.shields.io/badge/Rust-stable-000000.svg)](https://www.rust-lang.org)

</div>

---

LiteExif 使用 Tauri 2、React 和 Rust 构建，全部处理都在本地完成，照片不会离开你的电脑。图像排版与合成由 Rust 引擎执行，ExifTool 仅用于读取元数据。

## 功能特性

- **模板驱动水印**：用 JSON 模板定义排版，输出相机参数、品牌 Logo、圆角、阴影、留白与模糊背景。
- **内置 7 套模板**：`标准水印`、`标准水印2`、`右下角参数`、`logo居中`、`文件夹名+右下角参数`、`背景模糊`、`尼康专用背景模糊`，可在设置中新建与另存。
- **实时预览**：先完成与导出完全一致的全分辨率排版，再缩放显示；支持滚轮缩放、拖动平移、双击复位，重复预览命中缓存。
- **批量导出**：多选照片、逐张调度以限制内存，可跳过已存在文件并设置 JPEG 质量。
- **GPU 加速**：模糊计算使用经过 CPU 像素比对验证的 DX12（wgpu）加速，GPU 不可用时自动回退 CPU。
- **原生桌面体验**：无边框自定义标题栏、文件树、右键菜单（删除到回收站）。

## 下载安装

前往 [Releases](https://github.com/ptsfdtz/LiteExif/releases) 下载最新的 Windows 安装包（NSIS），运行后即可使用，无需单独安装 Python。

## 快速开始

1. 打开 LiteExif，在设置中选择输入目录与输出目录。
2. 在左侧勾选需要处理的照片。
3. 在预览区切换效果（数字 1–7），确认排版效果。
4. 点击右上角「开始处理」批量导出。

## 系统要求

- Windows 10/11（WebView2 运行时）
- 开发需要 Node.js 22 与 Rust stable

## 从源码构建

```powershell
pnpm install
pwsh ./scripts/fetch-exiftool.ps1
pnpm tauri dev
```

打包安装程序：

```powershell
pnpm desktop:build
```

安装包输出到 `src-tauri/target/release/bundle/nsis/`。

## 项目结构

- `src/`：React 与 TypeScript 桌面界面
  - `App.tsx`：主界面与状态编排
  - `components/`：预览、文件树、设置与对话框
- `src-tauri/src/`：Rust 引擎
  - `lib.rs`：Tauri 命令与事件
  - `engine.rs`：排版与合成主流程
  - `raster.rs`：光栅化与绘制
  - `text.rs`：基于 FreeType 的文字绘制
  - `gpu.rs`：wgpu/DX12 模糊加速
- `config/templates/`：水印处理模板（JSON）
- `config/fonts/`：水印字体（Roboto、阿里巴巴普惠体 2）
- `config/logos/`：相机品牌 Logo
- `scripts/`：ExifTool 获取与渲染对照脚本

## 实现说明

- 图像处理由 Rust 引擎完成。ExifTool 仅用于读取元数据，其发布基于 GPL v1 与 Artistic License 2.0。
- 渲染规则对齐 semi-utils：使用原生 FreeType 绘制文字，按 Pillow 的规则处理字体度量、裁边、透明图缩放、圆角和模糊。应用和安装包不需要 Python，FreeType 在 Windows 构建时静态编译。
- EXIF 缺失字段使用模板默认值，不自动借用其他字段补全。
- 文件树按需加载：初始只读取输入/输出目录的第一层，展开文件夹或点击「显示更多」时才分页读取下一批，避免 NAS 等网络目录一次性扫描整个文件树。
- 批处理逐张调度以限制内存，图像处理内部使用 Rayon 并行；模糊支持经过 CPU 像素比对验证的 DX12 加速。
- 预览先完成与导出相同的全分辨率排版，再缩小到最长边 1600 像素；第一次预览可能比低分辨率预览慢，重复预览使用缓存。

## 测试与验证

```powershell
pnpm build
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

## 贡献

欢迎提交 Issue 与 Pull Request。提交前请确保：

```powershell
pnpm build
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

均通过。新增或修改模板时，请同步更新参考图与测试。

## 许可证

项目基于 [Apache License 2.0](LICENSE) 发布。

渲染算法和字体依赖的来源与许可证见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
