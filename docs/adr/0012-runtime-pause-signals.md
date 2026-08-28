# ADR 0012：自动切换运行环境信号

- 状态：Accepted
- 日期：2026-08-28

## 背景

V2 自动切换需要在电池供电和前台全屏应用期间暂停。该能力属于平台状态，不应进入公共 Scheduler 的操作系统条件分支。当前 Windows 开发环境使用 Rust GNU ABI，直接增加新的 Windows import library 会要求额外的 `dlltool`，破坏已有的可复现构建入口。

## 决策

- 新增统一的 `PlatformEnvironmentService`，Scheduler 只读取 `RuntimeEnvironment`。
- Windows Adapter 仅在到期规则需要环境信号时，调用系统自带 PowerShell：通过 CIM 读取电池状态，并在隔离的进程内调用 `user32.dll` 检查前台窗口是否覆盖任一屏幕。输出只包含布尔值、本地星期和分钟，不记录进程名称、窗口标题或内容。
- macOS Adapter 使用 `pmset`、`date` 和 AppleScript Accessibility 属性读取对应信号。
- 未配置电池、全屏、日期或时间规则时不启动环境查询进程。
- 任一查询失败时保守地记录错误并推迟本次切换，不绕过用户配置。

## 影响

- 保持平台边界与本地隐私要求，不新增服务端或后台守护进程。
- Windows 环境查询只在规则到期时发生，避免常驻轮询成本。
- macOS 路径需要在 Apple Silicon 真机验证；可能受到 Accessibility 权限策略影响，不能以 Windows 结果代替。
