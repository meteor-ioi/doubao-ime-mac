#!/bin/bash

# Doubao Voice Input - 快速启动脚本 (macOS)

# 确保环境变量包含 Cargo
export PATH="$HOME/.cargo/bin:$PATH"

echo "🚀 正在检查环境..."

# 检查 Node.js
if ! command -v node &> /dev/null; then
    echo "❌ 错误: 未找到 Node.js，请先安装。"
    exit 1
fi

# 检查 Cargo
if ! command -v cargo &> /dev/null; then
    echo "❌ 错误: 未找到 Cargo (Rust)，请先安装。"
    exit 1
fi

echo "📦 正在启动开发环境 (Tauri dev)..."

# 启动项目
npx @tauri-apps/cli dev
