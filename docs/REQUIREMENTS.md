# 4K Wallpaper Desktop - V1 完整需求与开发规格

## 1. 项目目标

开发一款纯本地运行的跨平台 4K 壁纸桌面软件。

目标平台：

- Windows 10 / Windows 11
- macOS
- Windows 与 macOS 共用绝大部分业务代码
- 平台相关能力通过独立 Platform Adapter 实现

本项目为纯本地桌面应用，不建设任何自有服务器。

软件运行模式：

```text
用户电脑
  │
  ├── 本地 SQLite
  ├── 本地壁纸缓存
  ├── 本地配置
  ├── 本地定时任务
  │
  └── HTTPS
       ↓
    第三方壁纸 API
```

明确不包含：

- 自建服务器
- Web 后端
- 用户注册
- 用户登录
- 云同步
- Redis
- MySQL/PostgreSQL
- Docker
- CDN
- 对象存储
- SaaS
- 多租户
- 支付
- 动态壁纸
- AI 图片识别
- AI 语义搜索
- 在线插件市场
- 在线自动升级服务

---

# 2. 技术栈

必须采用以下技术栈。

## 2.1 桌面框架

```text
Tauri 2
```

## 2.2 前端

```text
Vue 3
TypeScript
Vite
Pinia
Vue Router
pnpm
```

UI 组件库允许根据实际开发情况选择成熟组件库，但不得引入重量级 Web 框架。

## 2.3 Core

```text
Rust Stable
Tokio
reqwest
serde
tracing
```

## 2.4 数据库

```text
SQLite
```

数据库为本地文件数据库。

不得要求用户额外安装任何数据库服务。

## 2.5 图片处理

使用 Rust 图片处理库完成：

- 图片信息读取
- 图片缩放
- 图片裁剪
- 格式识别
- 缩略图
- SHA-256 去重

## 2.6 壁纸来源

V1：

```text
WallhavenProvider
LocalProvider
```

必须采用 Provider 抽象，不得把 Wallhaven 逻辑直接写入业务 Service。

---

# 3. 核心架构

整体架构：

```text
┌───────────────────────────────┐
│       Vue 3 + TypeScript      │
│              UI               │
└──────────────┬────────────────┘
               │
          Tauri Command
               │
┌──────────────▼────────────────┐
│            Rust Core          │
│                               │
│  WallpaperService             │
│  WallpaperProvider            │
│  DownloadService              │
│  SearchService                │
│  SchedulerService             │
│  ImageProcessor               │
│  MonitorService               │
│  CacheService                 │
│  StorageService               │
│  SettingsService              │
└──────────────┬────────────────┘
               │
        Platform Adapter
        ┌──────┴───────┐
        ▼              ▼
     Windows         macOS
```

业务层不得直接调用 Windows/macOS API。

必须通过统一接口：

```text
PlatformWallpaperService
PlatformMonitorService
PlatformStartupService
PlatformTrayService
```

进行调用。

---

# 4. 推荐项目目录

项目至少按照以下职责拆分：

```text
wallpaper-desktop/
│
├── src/
│   ├── api/
│   ├── components/
│   ├── views/
│   ├── router/
│   ├── stores/
│   ├── models/
│   ├── composables/
│   └── utils/
│
├── src-tauri/
│   ├── src/
│   │   ├── commands/
│   │   ├── db/
│   │   ├── wallpaper/
│   │   ├── provider/
│   │   │   ├── wallhaven/
│   │   │   └── local/
│   │   ├── download/
│   │   ├── image/
│   │   ├── monitor/
│   │   ├── scheduler/
│   │   ├── cache/
│   │   ├── settings/
│   │   ├── platform/
│   │   │   ├── windows/
│   │   │   └── macos/
│   │   ├── models/
│   │   └── error/
│   │
│   └── Cargo.toml
│
├── migrations/
├── REQUIREMENTS.md
├── package.json
└── pnpm-lock.yaml
```

禁止把所有 Rust 代码堆积到 `main.rs` 或 `lib.rs`。

---

# 5. 本地数据目录

程序安装目录不得用于保存运行数据。

Windows：

```text
%LOCALAPPDATA%/<AppName>/
```

macOS：

```text
~/Library/Application Support/<AppName>/
```

内部结构：

```text
<AppData>/
│
├── data/
│   └── wallpaper.db
│
├── wallpapers/
│   └── original/
│
├── cache/
│   ├── thumbnails/
│   └── processed/
│
├── logs/
│
└── config/
```

所有路径必须通过系统 API 获取。

禁止硬编码：

```text
C:\Users\xxx
/Users/xxx
```

---

# 6. 在线壁纸 Provider

## 6.1 Provider 抽象

至少定义类似：

```rust
pub trait WallpaperProvider {

    fn provider_name(&self) -> &'static str;

    async fn latest(
        &self,
        query: WallpaperQuery
    ) -> Result<Vec<RemoteWallpaper>>;

    async fn search(
        &self,
        query: WallpaperQuery
    ) -> Result<Vec<RemoteWallpaper>>;

    async fn get_detail(
        &self,
        remote_id: &str
    ) -> Result<RemoteWallpaper>;

    async fn download(
        &self,
        wallpaper: &RemoteWallpaper
    ) -> Result<PathBuf>;
}
```

具体签名允许根据 Rust 实际情况优化，但职责不得改变。

## 6.2 统一查询模型

业务层不得使用 Wallhaven 专有参数。

定义统一查询模型：

```text
WallpaperQuery

keyword
category
minWidth
minHeight
aspectRatio
page
pageSize
sort
safety
```

Provider 自行转换为第三方 API 参数。

资源列表中的总数表示当前筛选条件下保存在本机 SQLite 的资源元数据索引数，不代表已下载同等数量的高清原图。列表总数超过 `pageSize` 时必须提供明确分页入口，并展示当前范围和总数，用户应能逐页查看全部稳定排序结果。

分页必须提供首页、上一页、下一页、尾页和指定页码跳转。发现页支持跨页批量选择并标记“不喜欢”；分类、搜索和收藏支持跨页批量移除索引；图库支持跨页批量移除图库资源。被标记不喜欢或移除索引的资源必须保留排除状态，后续在线同步或本地目录扫描不得重新展示。任何本地资源管理操作不得删除用户磁盘中的原始文件。

收藏列表是 `favorite = true` 的实时过滤结果。用户取消收藏后，该卡片和总数必须立即从收藏页面更新，不依赖重新进入页面或手动刷新。

---

# 7. WallhavenProvider

V1 默认在线资源源。

默认要求：

```text
最低分辨率：
3840 × 2160

内容：
SFW

优先：
横屏壁纸
```

支持：

- 最新
- 热门
- 随机
- 关键词搜索
- 分类
- 分辨率过滤
- 比例过滤
- 分页

至少映射：

```text
remoteId
provider
sourcePageUrl
originalUrl
thumbnailUrl
width
height
resolution
ratio
fileSize
mimeType
category
purity
tags
createdAt
```

第三方 API 不可用时不得导致整个应用崩溃。

---

# 8. 资源同步机制

资源同步和壁纸切换必须是两个独立功能。

## 8.1 在线资源同步

默认：

```text
每 24 小时执行一次
```

同时支持：

```text
手动刷新
```

应用启动后：

如果距离上一次成功同步超过 24 小时，可执行一次同步。

同步内容主要为：

```text
Metadata
Thumbnail
```

禁止每天自动批量下载大量 4K 原图。

同步流程：

```text
Wallhaven API
     ↓
获取 metadata
     ↓
provider + remoteId 去重
     ↓
保存 SQLite
     ↓
需要时加载 thumbnail
```

---

# 9. 原图下载

以下情况才下载原图：

1. 用户点击下载。
2. 用户点击设为壁纸。
3. 自动切换即将使用该壁纸。
4. 用户收藏并明确要求保存在本地。
5. 自动预加载少量即将使用的壁纸。

下载完成后：

```text
计算 SHA-256
     ↓
检查重复
     ↓
写入 original/
     ↓
更新 SQLite
```

不得因为 metadata 相同就重复保存图片。

---

# 10. 本地壁纸功能

提供：

```text
LocalProvider
```

用户可以选择本地目录。

例如：

```text
D:\Wallpapers
~/Pictures/Wallpapers
```

支持扫描：

```text
jpg
jpeg
png
webp
```

扫描后：

- 读取文件名
- 读取宽高
- 读取文件大小
- 计算 Hash
- 保存数据库索引

默认不得移动、修改或删除用户原始文件。

---

# 11. 壁纸数据库设计

至少包含以下数据模型。

## 11.1 wallpaper

建议字段：

```text
id

provider
remote_id

name
source_page_url
original_url
thumbnail_url

local_path

width
height
aspect_ratio

file_size
mime_type

category
purity

hash

download_status

favorite
blacklisted

created_at
synced_at
downloaded_at
last_used_at
```

唯一约束：

```text
provider + remote_id
```

本地文件可使用：

```text
hash
```

唯一性判断。

---

# 12. 标签

表：

```text
tag

id
name
```

关系：

```text
wallpaper_tag

wallpaper_id
tag_id
```

用于：

- 搜索
- 分类
- 展示

---

# 13. 显示器模型

保存：

```text
monitor

id
system_monitor_id
name
width
height
position_x
position_y
primary
last_seen_at
```

必须能够识别：

- 当前主屏幕
- 扩展屏
- 屏幕分辨率
- 屏幕位置

显示器拔出后不得删除用户配置。

重新插入后应尽可能恢复配置。

---

# 14. 多显示器壁纸

V1 必须支持以下两种模式。

## 14.1 所有屏幕使用同一壁纸

例如：

```text
Display 1 → A
Display 2 → A
```

每块屏幕可以根据自身分辨率单独生成适配后的图片。

---

## 14.2 每块屏幕独立壁纸

例如：

```text
Laptop
自然风景
30分钟切换

Monitor 2
动漫
1小时切换
```

每块显示器允许配置：

```text
enabled
wallpaperSource
category
changeInterval
fitMode
```

---

# 15. V1 不实现跨屏拼接壁纸

例如：

```text
一张超宽壁纸跨两块显示器拼接
```

归入 V2。

架构不得阻止未来添加该能力。

---

# 16. Windows 壁纸实现

Windows 平台优先使用系统提供的多显示器壁纸能力。

Platform Adapter：

```text
WindowsWallpaperService
```

职责：

```text
获取显示器
设置指定显示器壁纸
设置全部显示器壁纸
```

Windows Native API 调用必须与业务层隔离。

---

# 17. macOS 壁纸实现

macOS 通过系统原生桌面 API 为具体屏幕设置桌面图片。

Platform Adapter：

```text
MacOSWallpaperService
```

允许使用：

- Rust macOS binding
- Objective-C Bridge
- Swift Bridge

但最终业务接口必须和 Windows 一致。

---

# 18. 图片适配模式

V1 支持：

```text
Fill
Fit
Center
Stretch
```

默认：

```text
Fill
```

Fill：

```text
保持比例
→ 放大到覆盖屏幕
→ 居中裁剪
```

例如：

```text
原图
3840 × 2160

屏幕
3440 × 1440

↓

计算目标比例
↓

生成适配后的图片
```

输出存储：

```text
cache/processed/
```

---

# 19. 不直接修改原始壁纸

原图：

```text
wallpapers/original/
```

必须保持不变。

所有：

- Resize
- Crop
- Format conversion

结果存储到：

```text
cache/processed/
```

processed 文件允许自动删除和重新生成。

---

# 20. 自动换壁纸

自动换壁纸属于应用内部 Scheduler。

禁止引入：

- Quartz
- 外部任务服务
- 系统 Cron

支持：

```text
不启用
10 分钟
30 分钟
1 小时
2 小时
6 小时
12 小时
每天
每周
自定义
```

自定义周期最低不得小于：

```text
1 分钟
```

推荐内部保存：

```text
interval_seconds
last_change_time
next_change_time
```

---

# 21. Scheduler 行为

Scheduler 必须支持：

- 应用后台运行时持续工作
- 睡眠恢复后重新计算任务
- 错过执行时间后允许立即补执行一次
- 应用退出后停止
- 应用重新启动后根据数据库恢复状态

不得因为电脑睡眠：

```text
14:00
↓
睡眠
↓
18:00 唤醒
```

而连续执行几十次补偿任务。

最多补执行一次。

---

# 22. 自动壁纸选择算法

自动切换时：

```text
当前显示器配置
        ↓
获取指定来源
        ↓
过滤分类
        ↓
过滤黑名单
        ↓
过滤最近使用
        ↓
随机选择
        ↓
下载（如未下载）
        ↓
图片适配
        ↓
设置壁纸
        ↓
记录 history
```

尽量避免短时间重复使用同一壁纸。

---

# 23. 壁纸历史记录

建立：

```text
wallpaper_history
```

至少记录：

```text
id
wallpaper_id
monitor_id
used_at
trigger_type
```

trigger_type：

```text
MANUAL
SCHEDULE
```

用于：

- 避免重复
- 查看最近使用
- 排查问题

---

# 24. 分类

V1 至少支持：

```text
全部
自然
动漫
人物
本地
收藏
```

Wallhaven 原生类别允许映射到统一分类。

未来 Provider 不得依赖 Wallhaven 分类定义。

---

# 25. 搜索

V1 为 Metadata 搜索。

支持：

```text
壁纸名称
文件名
标签
分类
Provider
```

支持模糊匹配。

例如：

```text
mountain
雪山
sunset
car
anime
```

V1 不进行：

```text
图片视觉识别
CLIP
Embedding
向量数据库
AI语义搜索
```

这些功能归 V2。

---

# 26. 收藏

每张壁纸允许：

```text
收藏
取消收藏
```

收藏壁纸：

- 不参与自动缓存清理。
- 已下载原图不得自动删除。

---

# 27. 黑名单 / 不喜欢

提供：

```text
不喜欢
```

被标记后：

```text
blacklisted = true
```

以后：

- 自动切换不得选择。
- 推荐列表默认隐藏。
- 搜索可选择是否显示。

---

# 28. 缓存系统

默认最大缓存：

```text
5 GB
```

设置支持：

```text
1 GB
5 GB
10 GB
20 GB
无限制
```

自动清理策略：

```text
LRU
```

优先删除：

1. processed 文件。
2. 长时间未使用原图。
3. 非收藏文件。

禁止自动删除：

```text
favorite = true
```

的原图。

---

# 29. 缩略图

壁纸列表必须优先加载：

```text
Thumbnail
```

不得在壁纸浏览页面直接加载大量 4K 原图。

目标：

```text
100+ 壁纸列表
```

正常滚动时不能因为加载原图造成明显卡顿。

建议使用：

```text
Lazy Loading
Virtual List / Virtual Grid
```

之一。

---

# 30. UI 页面

V1 至少包含以下页面。

## 30.1 首页 / Discover

展示：

- 推荐壁纸
- 最新壁纸
- 随机壁纸
- 手动刷新

---

## 30.2 分类

显示：

```text
全部
自然
动漫
人物
本地
收藏
```

---

## 30.3 搜索

顶部：

```text
搜索框
```

支持：

```text
关键词
分类
分辨率
来源
收藏状态
```

---

## 30.4 壁纸详情

至少显示：

```text
大图预览
名称
尺寸
来源
标签
文件大小
```

操作：

```text
设为壁纸
选择显示器
下载
收藏
不喜欢
删除本地缓存
```

---

# 31. 显示器设置页面

显示类似：

```text
┌────────────┐
│ Laptop     │
│ 2560×1600  │
│ Primary    │
└────────────┘

┌────────────┐
│ Monitor 2  │
│ 3840×2160  │
└────────────┘
```

允许配置：

```text
统一模式
独立模式
```

独立模式允许每屏配置：

```text
壁纸来源
分类
切换周期
选择方式（轮询 / 随机）
Fit Mode
```

独立模式的配置必须按显示器 ID 分别持久化。切换导航页面、重新检测显示器或重启应用后，界面必须回填该显示器实际保存的配置，不得使用另一显示器的值或固定默认值覆盖。

---

# 32. 设置页面

至少包含：

## 常规

```text
开机启动
关闭窗口时最小化到托盘
```

## 自动切换

```text
默认周期
```

## 资源库

```text
自动同步
同步间隔
手动刷新
```

V1 默认同步周期：

```text
24 小时
```

## 缓存

```text
缓存大小
当前缓存占用
清理缓存
```

## 本地图库

```text
添加目录
删除目录
重新扫描
```

---

# 33. 系统托盘

应用必须支持系统托盘。

托盘菜单至少包含：

```text
打开主窗口

下一张壁纸

暂停自动切换

恢复自动切换

退出
```

---

# 34. 窗口关闭行为

默认：

```text
点击 X
↓
隐藏主窗口
↓
程序继续驻留托盘
```

设置允许用户改为：

```text
点击 X
↓
完全退出
```

---

# 35. 开机启动

提供设置：

```text
开机自动启动
```

默认：

```text
false
```

Windows/macOS 使用各自平台支持的启动机制。

业务层不得直接操作注册表等平台实现。

---

# 36. 网络异常

必须能够处理：

```text
没有网络
API超时
DNS失败
HTTP 429
HTTP 500
下载中断
非法图片
第三方 API 返回错误
```

网络错误不得：

- 导致应用退出。
- 导致 Scheduler 崩溃。
- 破坏 SQLite。

已有本地壁纸时：

```text
没有网络
↓
仍然正常自动换本地壁纸
```

---

# 37. 下载策略

HTTP 请求必须：

```text
设置超时
```

必须限制：

```text
并发下载数量
```

建议最大：

```text
3
```

不允许同时开启几十个 4K 文件下载任务。

---

# 38. 数据库 Migration

数据库必须使用 Migration。

禁止：

```text
程序启动时散落大量 CREATE TABLE IF NOT EXISTS
```

数据库版本升级必须可维护。

第一版至少提供：

```text
V1 initial migration
```

---

# 39. 日志

Rust 使用：

```text
tracing
```

日志保存：

```text
<AppData>/logs/
```

日志至少包含：

```text
程序启动
程序退出
数据库初始化
Provider同步
下载失败
下载完成
Scheduler执行
显示器变化
设置壁纸失败
缓存清理
异常
```

不得记录：

- 图片二进制。
- 敏感 Token。
- 大量无意义 debug 内容。

---

# 40. 错误模型

建立统一：

```text
AppError
```

至少分类：

```text
Network
Database
FileSystem
Image
Provider
Platform
Wallpaper
Monitor
Configuration
Unknown
```

禁止业务代码大量使用：

```rust
unwrap()
expect()
```

导致程序直接 Panic。

可恢复错误应转换为 Result。

---

# 41. 隐私要求

本软件为纯本地软件。

不得加入：

```text
Telemetry
用户行为上传
设备信息上传
壁纸历史上传
日志上传
Crash 自动上传
```

用户数据只保存在本地。

程序只允许访问：

- 用户明确选择的本地目录。
- 应用自己的数据目录。
- 壁纸 Provider 网络接口。

---

# 42. API Key

如果 Provider 需要 API Key：

不得：

```text
硬编码到源码
提交到 Git
```

应保存：

```text
本地配置
```

如果 Wallhaven 普通 SFW 功能无需 Key，则优先使用无 Key 模式。

---

# 43. 性能要求

应用后台空闲时：

- 不允许持续高 CPU。
- 不允许高频轮询 API。
- 不允许不断扫描硬盘。

Scheduler 应采用合理 Timer。

壁纸列表：

```text
500+ metadata
```

时应保持可正常使用。

SQLite 查询必须分页。

---

# 44. 图片安全

下载完成后必须：

```text
确认 HTTP 成功
↓
检查 Content-Type 或文件格式
↓
尝试读取图片
↓
确认图片有效
↓
写入数据库
```

无效文件不得设置为壁纸。

临时文件：

```text
xxx.tmp
```

下载成功后再原子重命名。

---

# 45. Tauri Command

建议至少提供：

```text
get_monitors

list_wallpapers
search_wallpapers
get_wallpaper_detail

sync_wallpapers

download_wallpaper
delete_wallpaper_cache

set_wallpaper
set_wallpaper_for_monitor

favorite_wallpaper
unfavorite_wallpaper
blacklist_wallpaper

add_local_directory
remove_local_directory
scan_local_directory

get_settings
update_settings

get_cache_info
clear_cache

get_scheduler_status
pause_scheduler
resume_scheduler
trigger_next_wallpaper
```

具体名称允许调整，但能力不得缺失。

---

# 46. 状态管理

Vue 层至少拆分：

```text
wallpaperStore
monitorStore
settingsStore
schedulerStore
```

禁止所有页面自行重复调用底层逻辑。

---

# 47. V1 页面导航

建议：

```text
Discover

Categories

Search

Favorites

Local

Displays

Settings
```

采用左侧导航布局。

---

# 48. UI 风格

目标：

```text
现代
简洁
图片优先
深浅背景均具有良好视觉效果
```

壁纸卡片至少展示：

```text
Thumbnail
Resolution
Favorite
```

鼠标 Hover 可显示快捷：

```text
设为壁纸
下载
收藏
```

UI 不应设计成传统后台管理系统。

禁止大量：

```text
表格
表单
CRUD 管理页面
```

壁纸浏览必须以图片网格为核心。

---

## 48.1 V1 应用主题

V1 必须支持：

```text
深色
浅色
跟随系统
自定义配色
纯色背景
渐变背景
彩虹背景
```

主题配置只保存在本机，并在应用启动时恢复。

自定义配色至少允许设置：

```text
强调色
辅助色
背景色
卡片色
```

主题必须通过统一 Design Token / CSS Variable 实现，不得把颜色判断散落到业务组件或 Rust Core。

跟随系统模式必须响应 Windows/macOS 外观变化，并始终使用应用内置的对应浅色/深色默认配色和纯色背景；该模式不允许设置背景颜色或其他背景效果。彩虹背景应尊重系统“减少动态效果”偏好。

应用识别到当前主题主体颜色为浅色时，必须自动使用足够深的正文、标签、说明和控件文字；主题预览也必须根据预览背景亮度自动选择可读的前景色。

V1 主题只改变视觉配色和背景效果，不改变页面结构、功能入口和导航布局。不同形态的应用外观与导航主题归入 V2。

---

# 49. 第一次启动

首次启动流程：

```text
启动程序
↓
创建 AppData
↓
初始化数据库
↓
检测显示器
↓
加载默认配置
↓
显示主页面
↓
异步获取在线资源
```

UI 不得因为第一次资源同步长时间白屏。

---

# 50. 默认配置

建议：

```text
online_provider = wallhaven

minimum_resolution = 3840x2160

safety = SFW

resource_sync_enabled = true

resource_sync_interval = 24h

wallpaper_auto_change = false

wallpaper_change_interval = 30min

wallpaper_fit_mode = fill

cache_limit = 5GB

close_to_tray = true

auto_start = false

theme_mode = dark

theme_effect = solid
```

---

# 51. 断网模式

如果数据库已有数据：

```text
启动
↓
检测网络失败
↓
显示已有 metadata
↓
显示本地缩略图
↓
本地壁纸继续工作
```

不能因为 Wallhaven 不可访问而无法启动软件。

---

# 52. 显示器热插拔

程序运行期间需要正确处理：

```text
连接外接显示器
拔出外接显示器
改变主显示器
改变分辨率
```

至少应：

```text
重新检测 Monitor
↓
更新状态
↓
保持已有配置
```

不得崩溃。

---

# 53. 睡眠/唤醒

睡眠恢复后：

```text
重新检测显示器
重新计算 Scheduler
```

必要时补执行：

```text
最多一次
```

壁纸任务。

---

# 54. 打包

Windows：

生成本地可安装版本。

macOS：

生成：

```text
.app
```

以及可用安装产物。

当前项目为个人本地使用：

V1 不要求：

```text
Windows商业代码签名
Apple Developer ID
Notarization
```

架构不得依赖签名才能运行核心功能。

---

# 55. 构建要求

至少支持：

开发：

```bash
pnpm tauri dev
```

生产：

```bash
pnpm tauri build
```

README 必须包含：

```text
开发环境
安装依赖
启动开发环境
构建
数据目录
日志目录
```

---

# 56. 测试

Core 层优先编写单元测试。

至少覆盖：

```text
WallpaperQuery转换
图片适配尺寸计算
Hash
缓存 LRU
Scheduler 下一执行时间
数据库 CRUD
Provider response mapping
搜索逻辑
```

平台 API 难以单元测试的部分允许进行 Integration Test / Manual Test。

---

# 57. 必须重点人工测试的场景

## 单屏

```text
1920×1080
2560×1440
3840×2160
```

## 多屏

例如：

```text
Laptop 2560×1600
+
External 3840×2160
```

测试：

```text
相同壁纸
不同壁纸
不同分类
不同周期
```

## 显示器变化

测试：

```text
拔掉外接显示器
重新插入
切换主屏
修改分辨率
```

---

# 58. 图片测试

准备不同图片：

```text
1920×1080
2560×1440
3840×2160
5120×2880
7680×4320

16:9
16:10
21:9
32:9

横图
竖图
```

验证：

```text
Fill
Fit
Center
Stretch
```

---

# 59. V1 验收标准

以下全部通过才算 V1 完成。

## AC-01

应用可以在目标系统正常启动。

---

## AC-02

首次运行自动创建：

```text
SQLite
缓存目录
日志目录
配置
```

---

## AC-03

可以从 Wallhaven 获取 ≥4K 的 SFW 壁纸 metadata。

---

## AC-04

可以浏览在线壁纸缩略图。

---

## AC-05

可以关键词搜索壁纸。

---

## AC-06

可以根据分类筛选。

---

## AC-07

可以下载一张在线壁纸到本地。

---

## AC-08

重复下载相同壁纸不会生成重复文件。

---

## AC-09

可以手动将壁纸设置为当前桌面。

---

## AC-10

可以检测当前所有显示器。

---

## AC-11

双显示器可以使用同一张壁纸。

---

## AC-12

双显示器可以分别设置不同壁纸。

---

## AC-13

不同分辨率显示器能够生成适合自身尺寸的壁纸。

---

## AC-14

支持：

```text
10分钟
30分钟
1小时
每天
每周
自定义
```

自动切换。

---

## AC-15

不同屏幕能够拥有独立自动切换配置。

---

## AC-16

断网以后已下载壁纸仍然可以正常自动切换。

---

## AC-17

支持收藏。

---

## AC-18

支持黑名单。

---

## AC-19

收藏壁纸不得被缓存清理删除。

---

## AC-20

缓存超过配置上限以后可以自动执行清理。

---

## AC-21

支持本地目录扫描。

---

## AC-22

本地图片可以直接设置为桌面。

---

## AC-23

支持系统托盘。

---

## AC-24

窗口关闭以后默认驻留托盘。

---

## AC-25

托盘可以：

```text
下一张
暂停
恢复
打开
退出
```

---

## AC-26

支持可选开机自动启动。

---

## AC-27

显示器插拔不会导致程序崩溃。

---

## AC-28

电脑睡眠恢复后 Scheduler 可以继续运行。

---

## AC-29

Provider 请求失败不会导致程序崩溃。

---

## AC-30

可以完成：

```text
pnpm tauri build
```

并生成本地可安装程序。

---

# 60. 开发阶段

Codex 必须按照以下阶段推进。

## Phase 0：项目初始化

完成：

```text
Tauri 2
Vue 3
TypeScript
Rust
SQLite
基础目录
日志
错误模型
```

验收：

```text
pnpm tauri dev
```

正常。

---

# Phase 1：平台核心能力

优先开发：

```text
获取显示器
设置本地图片为壁纸
指定显示器设置壁纸
```

必须先验证：

```text
Windows/macOS系统壁纸能力可用
```

再继续开发大量 UI。

---

# Phase 2：数据库

完成：

```text
Migration
Wallpaper
Tag
Monitor
Settings
History
```

---

# Phase 3：Provider

完成：

```text
WallpaperProvider
WallhavenProvider
LocalProvider
```

实现：

```text
latest
search
detail
download
```

---

# Phase 4：图片处理

完成：

```text
image metadata
resize
crop
Fill
Fit
Center
Stretch
thumbnail
hash
```

---

# Phase 5：壁纸 Core

实现：

```text
下载
去重
处理
设置
History
Favorite
Blacklist
```

---

# Phase 6：Scheduler

实现：

```text
自动切换
多屏独立周期
Pause
Resume
睡眠恢复
```

---

# Phase 7：UI

实现：

```text
Discover
Categories
Search
Favorites
Local
Displays
Settings
```

---

# Phase 8：缓存

实现：

```text
容量统计
LRU
自动清理
手动清理
收藏保护
```

---

# Phase 9：桌面集成

实现：

```text
Tray
Close to Tray
Auto Start
```

---

# Phase 10：打包

完成：

```text
Windows Build
macOS Build
README
```

---

# 61. Codex 开发约束

Codex 在开发过程中必须遵守以下规则。

## 61.1 不擅自增加服务器

禁止引入：

```text
Spring Boot
Node Server
Express
NestJS
ASP.NET
云函数
```

所有逻辑必须在本地程序中完成。

---

## 61.2 不擅自更换技术栈

未经明确要求不得将：

```text
Tauri
Vue
Rust
SQLite
```

替换成：

```text
Electron
Flutter
JavaFX
React Native
```

---

## 61.3 不为了“以后可能需要”过度设计

V1 不实现：

```text
微服务
事件总线
MQ
复杂DDD
CQRS
云配置
```

模块边界清晰即可。

---

## 61.4 Platform 隔离

禁止出现大量：

```rust
if windows {
}

if macos {
}
```

散落在业务代码。

平台能力集中：

```text
platform/windows
platform/macos
```

---

## 61.5 Provider 隔离

业务代码禁止直接依赖：

```text
WallhavenResponse
WallhavenQuery
```

必须转换为项目统一 Domain Model。

---

## 61.6 每个阶段必须可运行

禁止一次修改整个项目后最后才编译。

每完成一个 Phase：

```text
cargo check
pnpm build
相关 test
```

必须通过。

发现问题必须立即修复。

---

## 61.7 不允许留大量 TODO 作为“完成”

核心需求不得通过：

```text
TODO
Mock
Stub
fake implementation
```

冒充完成。

---

# 62. V2 预留，但禁止 V1 实现

以下只需要架构允许未来扩展：

```text
AI图片内容识别

AI语义搜索

Embedding

跨屏超宽壁纸

动态壁纸

视频壁纸

更多 Wallpaper Provider

壁纸源插件

自动升级

云同步
```

V1 不投入开发时间。

---

# 63. 最终交付内容

Codex 最终必须交付：

```text
完整源代码

Windows 可构建代码

macOS 可构建代码

SQLite Migration

README.md

构建说明

运行说明

测试说明

核心功能测试

生产构建成功
```

同时 README 中明确标注：

```text
应用数据目录
缓存目录
日志目录
数据库目录
```

---

# 64. 最终项目原则

整个项目始终遵守：

```text
Local First

No Server

Provider Independent

Platform Independent Core

Native Platform Adapter

Offline Available

Low Resource Usage

Simple Architecture

Stable Before Fancy UI
```

开发优先级：

```text
平台壁纸能力
>
多显示器
>
资源下载
>
图片处理
>
Scheduler
>
数据管理
>
UI美化
```

任何情况下，不能为了 UI 完成度牺牲底层壁纸、多显示器和 Scheduler 的可靠性。
