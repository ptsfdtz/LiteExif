# Third-Party Notices

LiteExif 基于 Apache License 2.0 发布。本文件列出随应用分发或在构建时引用的第三方组件来源与许可证。完整许可证文本见 `licenses/` 目录（随安装包分发）。

## 渲染算法参考

- **semi-utils**（上游 Python 实现，LiteExif 的 Rust 渲染引擎与其逐像素对齐：FreeType 文字绘制、Pillow 规则的字体度量/裁边/透明图缩放/圆角/模糊）
  - 许可证：Apache License 2.0
  - 文本：`licenses/semi-utils.txt`
- **Pillow**（对照基准：Windows Pillow 12.1.0，BASIC 字体排版；合成采用 `Image.paste(source, box, mask=source)` 语义）
  - 许可证：MIT-CMU License
  - 文本：`licenses/Pillow.txt`

## 字体引擎

- **FreeType**（原生字体光栅化，Windows 构建时静态编译）
  - 许可证：FreeType License（BSD 风格）
  - 文本：`licenses/FreeType.txt`
- **freetype-rs**（Rust 绑定）
  - 许可证：MIT License
  - 文本：`licenses/freetype-rs.txt`

## 元数据读取

- **ExifTool**（Phil Harvey，仅用于读取照片 EXIF，不参与图像渲染）
  - 许可证：GPL version 1 与 Artistic License 2.0（以 ExifTool 自带声明为准）
  - 来源：https://exiftool.org/
  - 随应用分发于 `exiftool/` 目录

## 随附字体与 Logo

- **Roboto**（`config/fonts/Roboto-*.ttf`）
  - 许可证：Apache License 2.0
- **阿里巴巴普惠体 2**（`config/fonts/AlibabaPuHuiTi-2-*.otf`）
  - 以字体厂商发布的许可为准，请查阅阿里巴巴普惠体官方授权说明
- **相机品牌 Logo**（`config/logos/`）
  - 各 Logo 商标权归各自厂商所有，仅用于水印品牌标识

## Rust / Node 依赖

Rust 依赖（`src-tauri/Cargo.toml`，版本锁定见 `src-tauri/Cargo.lock`）与 Node 依赖（`package.json`，版本锁定见 `pnpm-lock.yaml`）各自遵循其上游许可证（多为 MIT / Apache-2.0），以各包自带声明为准。
