# Doubao Voice Input (豆包语音输入)

一款专为 macOS 打造的轻量级语音输入工具，基于豆包 ASR 实现高精度、低延迟的实时语音转文本。

> [!NOTE]
> 本项目已全面转型为纯 macOS 版本。Windows 版本的遗留代码及工具已归档至 [legacy_windows](./legacy_windows) 目录。

## 🎨 功能特性

- 🎤 **实时流式识别** - 基于豆包 ASR 引擎，支持中英文混说，识别精度高。
- ⌨️ **快捷键唤起** - 双击 `Ctrl` 键即可快速开启或停止语音输入。
- 📍 **智能光标追踪** - 识别指示器跟随文本输入光标（Caret）动态定位，直观反馈状态。
- 🔄 **增量更新优化** - 采用智能算法仅更新变动文本，彻底告别打字时的文本闪烁。
- 🖥️ **系统托盘** - 轻量化托盘控制，支持隐藏应用图标及开机自启配置。
- 📦 **极致轻量** - 基于 Rust 和 Tauri v2 开发，单文件运行，启动极速。

## 🚀 快速开始

### 安装使用

1. 从 [Releases](https://github.com/EvanDbg/doubao-ime-win/releases) 下载最新的 `.dmg` 安装包。
2. 打开并将 **语音输入法** 拖动到应用程序文件夹 (Applications)。
3. 首次启动时，请按照系统提示授予 **辅助功能 (Accessibility)** 权限，以便程序模拟文本输入及追踪光标位置。

### 使用方法

1. **语音输入**:
   - 快速双击 `Ctrl` 键开始录音。
   - 说话时，识别结果将实时插入到当前光标所在位置。
   - 再次双击 `Ctrl` 或切换窗口即可停止并完成识别。

2. **状态指示**:
   - 识别时，光标下方会出现紫色胶囊指示器：
     - 🟣 紫色呼吸 = 正在录音...

3. **设置**:
   - 通过系统托盘图标右键菜单进入“设置”。
   - 可自定义快捷键、设置开机自启或隐藏 Dock 图标。

## 🛠 开发与构建

### 环境要求

- **macOS** (支持 Intel & Apple Silicon)
- **Rust** 1.75+
- **Node.js** (用于 Tauri 前端构建)

### 构建步骤

```bash
# 克隆项目
git clone https://github.com/EvanDbg/doubao-ime-win.git
cd doubao-ime-win

# 安装依赖并打包 (生成 .dmg)
sh build_mac.sh
```

## 📐 技术架构

| 模块 | 实现技术 |
|------|------|
| **核心语言** | Rust (Tokio 异步运行时) |
| **GUI 框架** | Tauri v2 (Vanilla HTML/JS 前端) |
| **语音协议** | 豆包 ASR (WebSocket + Protobuf) |
| **音频采集** | cpal |
| **系统交互** | macOS Accessibility API (AXUIElement) |
| **打包分发** | GitHub Actions (Auto DMG Release) |

## ⚖️ 免责声明

本项目基于豆包输入法客户端协议分析实现，非官方授权 API，仅供技术研究分享使用。请在遵守相关法律法规的前提下合理使用。

## 📄 许可证

[MIT License](./LICENSE)
