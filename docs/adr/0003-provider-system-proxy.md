# ADR 0003: Provider HTTP 遵循系统代理

- 状态：已接受
- 日期：2026-08-20

## 背景

Windows 真机验证中，Wallhaven 可通过系统网络配置访问，但直接 TCP 连接被当前网络环境阻断。`reqwest` 关闭默认特性后不会自动启用系统代理发现，导致 Provider 在 10 秒连接超时后返回 `Network` 错误。

## 决策

继续使用既定 Rust、Tauri 2 与 WallhavenProvider 技术边界，为 `reqwest` 启用官方 `system-proxy` 特性，同时保留 Rustls、连接超时、总请求超时和统一 `AppError` 映射。

## 影响

- Windows 与 macOS Provider 会遵循用户现有的系统代理配置。
- 不保存、上传或自行修改代理配置。
- 新增少量平台代理发现依赖；网络不可用时仍以可恢复错误返回，不影响本地目录与已缓存预置内容。
