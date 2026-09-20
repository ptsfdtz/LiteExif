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
- **高性能**：在与 semi-utils 逐像素一致的前提下，整帧（600 万像素）处理速度约为其 4 倍，详见[性能](#性能)。
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

## 发布与自动更新

推送 `v*` 形式的标签会触发 `.github/workflows/release.yml`：先运行 CI 校验，再按标签自动同步 `package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 的版本号，然后在 Windows runner 上构建 NSIS 安装包并自动创建或更新对应的 GitHub Release（含 Tauri 更新产物与 `latest.json`）。

```powershell
git tag v0.2.0
git push origin v0.2.0
```

本地也可以手动同步与校验版本号：

```powershell
pnpm version:set 0.2.0
pnpm version:check
```

仓库需要在 GitHub Secrets 中配置 Tauri 更新签名私钥（与 LiteMark 共用同一把密钥）：

- `TAURI_SIGNING_PRIVATE_KEY_V3`：私钥内容
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD_V3`：私钥密码

本地执行 `pnpm desktop:build` 时同样需要设置 `TAURI_SIGNING_PRIVATE_KEY`，否则无法生成更新签名产物。

应用启动后会自动向 `releases/latest/download/latest.json` 检查更新，发现新版本时弹出提示，用户确认后自动下载、安装并重启；标题栏的下载图标也可以手动检查更新。

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

## 性能

LiteExif 的渲染目标是与 semi-utils（Python + Pillow）逐像素一致，因此性能可以在同一画质标准下直接对比：所有基准输出与 Pillow 的最大通道误差都是 **0**，即「同画质下的耗时」。基准覆盖 7 套模板的横竖版，以及文字、缩放、模糊、圆角阴影等单项算子，输入为 3000×2000 / 2000×3000（约 600 万像素）的照片。

```powershell
# 生成基准需要 semi-utils 检出与 Python 环境（见下节）
.\.venv-renderer\Scripts\python.exe scripts/render-reference.py C:/path/to/semi-utils --benchmark
$env:LITEEXIF_PARITY_DIR = (Resolve-Path tmp/render-benchmark).Path
cargo test --manifest-path src-tauri/Cargo.toml --lib compare_semi_utils_reference -- --ignored
```

参考环境为 Windows 11 + RTX 3070 Laptop（dev profile，`opt-level = 2`）。全部 33 个用例的 Rust 总耗时约为 Pillow 的 **1/4**：

| 用例（600 万像素） | semi-utils | 本轮优化前 | 本轮优化后 | 相对 Pillow |
| --- | ---: | ---: | ---: | ---: |
| 背景模糊 | 1446.9 ms | 814.0 ms | **300.2 ms** | 0.21× |
| 标准水印2 | 1381.2 ms | 546.3 ms | **313.8 ms** | 0.23× |
| 尼康专用背景模糊 | 1045.3 ms | 649.9 ms | **316.1 ms** | 0.30× |
| 标准水印 | 719.1 ms | 225.2 ms | **160.0 ms** | 0.22× |
| 圆角 + 阴影 | 344.5 ms | 219.5 ms | **66.5 ms** | 0.19× |
| 高斯模糊 r=35 | 154.8 ms | 94.1 ms | **50.9 ms** | 0.33× |
| 缩放 → 1111×739 | 92.8 ms | 64.6 ms | **33.2 ms** | 0.36× |
| **合计（33 例）** | **13690 ms** | **6732 ms** | **3486 ms** | **0.26×** |

### 如何做到

优化集中在「别让每一帧都逐像素做重复工作」这一条主线上：

- **合成与拷贝走原始切片**：`alpha_over`、`blit`、`crop_with_padding`、`margin` 从逐像素 `get_pixel` / `put_pixel` 改成按行 `memcpy`，并对 `alpha == 0/255` 走快速分支；大画布的纯色填充（`filled_image`）用倍增 `memcpy` 替代 `RgbaImage::from_pixel`，把逐像素写变成按行拷贝。
- **缩放（Lanczos）**：全不透明照片跳过 premultiply / unpremultiply 两整帧（对照片是恒等操作）；水平与垂直重采样改成连续内存访问 + 累加缓冲，去掉每个通道重复的索引与边界计算。
- **模糊与阴影**：CPU 盒式模糊把每通道重复的 `min` / `saturating_sub` 索引计算提到像素级；阴影的 `alpha^1.5` 用 256 项查找表替代逐像素 `powf`。
- **文字**：FreeType 字形从「DEFAULT 量测 + RENDER 绘制」两次加载合并为一次 RENDER 加载，位图一次性拷出后再排版。
- **裁剪扫描**：`foreground_bbox` 从四个方向各扫一遍改为单遍统计行列是否含前景，并对开方加阈值提前退出。

### 大图像处理

- **GPU 加速**：模糊在图像达到 12.8 万像素且半径 ≥ 2 时走 DX12（wgpu）计算着色器，6 趟盒式模糊与 Pillow 完全一致；启动时用 CPU 结果逐字节校验，校验不通过或运行时出错会自动回退 CPU。
- **Rayon 并行**：缩放、模糊、填充与合成均按行并行，充分利用多核。
- **内存控制**：批处理逐张调度以限制峰值内存；单张内部的大图层（阴影、对齐画布）直接复用原始切片，避免中间克隆。
- 结果是整帧 600 万像素的模板（背景模糊、标准水印2 等）从 1.4 s 级降到约 0.3 s，而预览与导出走同一套全分辨率管线，所见即所得。

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
