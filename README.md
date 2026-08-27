# 4K Wallpaper Desktop

一款轻量、无广告、纯本地运行的跨平台 4K 壁纸桌面应用。项目使用 Tauri 2、Vue 3、TypeScript、Rust 和 SQLite，支持从 Wallhaven 浏览高清壁纸，也能安全管理用户自己的本地图库。

## V1 功能

- 推荐、最新、随机、分类和 Metadata 模糊搜索
- 30 张离线预设缩略图，高清原图按需下载
- 收藏、黑名单、下载去重和历史记录
- 跨页浏览、页码跳转、批量移除索引和“不喜欢”排除
- Windows/macOS 平台隔离的显示器与壁纸服务
- 多显示器独立壁纸、适配模式与自动轮换周期
- 本地目录扫描，不移动、修改或删除用户原文件
- 5 GB 默认缓存上限、LRU 自动清理、手动清理和收藏保护
- 系统托盘、关闭到托盘，以及登录后仅驻留托盘的开机启动设置
- 深色、浅色、跟随系统、自定义配色、渐变和彩虹应用主题

## 下载安装

Windows 11 x64 用户可从 [GitHub Releases](https://github.com/SlyLu/4K-Wallpaper-Desktop/releases/latest) 下载最新的 `x64-setup.exe` 安装包。应用目前未购买商业代码签名证书，首次安装时 Windows 可能显示来源提醒。

macOS Apple Silicon 安装产物必须在对应真机完成构建和验证后再发布。

## 开发环境

- Windows 11 x86_64，或 macOS Apple Silicon
- Node.js 20+
- pnpm 11+
- Rust stable
- Tauri 2 平台依赖
  - Windows：WebView2 与可用的 MSVC/MinGW 原生工具链
  - macOS：Xcode Command Line Tools

当前 Windows GNU 验证环境通过 `scripts/tauri.ps1` 自动加入用户级 MinGW64 工具链路径。

## 安装依赖

```bash
pnpm install
```

## 启动开发环境

```bash
pnpm tauri dev
```

## 检查与测试

```bash
pnpm typecheck
pnpm build
cd src-tauri
cargo fmt --all -- --check
cargo check
cargo test
```

## 生产构建

```bash
pnpm tauri build
```

Windows 首次执行 Rust 检查或 Tauri 构建时，如果本地缺少
`WebView2Loader.dll`，构建脚本会从微软官方 NuGet 源下载与当前 Rust
依赖匹配的固定版 WebView2 SDK，并在 SHA-256 校验通过后提取 x64 DLL。
后续构建会复用已验证的本地文件。

Windows 构建产物位于 `src-tauri/target/release/bundle/`。macOS 构建需要在 macOS 主机运行相同命令，并生成 `.app` 和对应安装产物。V1 不要求商业代码签名、Developer ID 或 notarization。

## 本地数据

应用不会建设或连接自有服务端，用户配置和状态只保存在本机。

- Windows 数据目录：`%LOCALAPPDATA%\4K Wallpaper Desktop`
- macOS 数据目录：`~/Library/Application Support/4K Wallpaper Desktop`
- SQLite：数据目录下的 `data/wallpaper.db`
- 配置：数据目录下的 `config/settings.json`
- 原图：数据目录下的 `wallpapers/original/`
- 缓存：数据目录下的 `cache/`
- 日志：数据目录下的 `logs/wallpaper-desktop.log`

LocalProvider 只索引用户明确选择的目录，原始图片保持原位且不会被缓存清理删除。

## 架构边界

- Windows 原生实现：`src-tauri/src/platform/windows/`
- macOS 原生实现：`src-tauri/src/platform/macos/`
- 公共业务只依赖 `PlatformMonitorService` 和 `PlatformWallpaperService`
- Provider、Scheduler、Wallpaper Core 和 Cache Service 不直接依赖平台原生 API

产品和技术基线以 `docs/REQUIREMENTS.md` 为准。

V2 功能规划见 `docs/REQUIREMENTS_V2_DRAFT.md`；该文件当前为待评审草案，不替代 V1 实施基线。
