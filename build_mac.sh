#!/bin/bash

# 语音输入法 - macOS 打包脚本 (优化版)
set -e # 出错即退出

# 记录开始时间
START_TIME=$(date +%s)

# 确保环境变量包含 Cargo
export PATH="$HOME/.cargo/bin:$PATH"

echo "🚀 开始打包 macOS DMG..."

# 1. 检查必要环境
echo "🔍 检查环境..."
if ! command -v node &> /dev/null; then
    echo "❌ 错误: 未找到 Node.js，请先安装。"
    exit 1
fi

if ! command -v cargo &> /dev/null; then
    echo "❌ 错误: 未找到 Cargo (Rust)，请先安装。"
    exit 1
fi

# 2. 版本同步
echo "🔄 同步版本信息..."
VERSION=$(grep -m 1 '^version =' Cargo.toml | cut -d '"' -f 2)
if [ -z "$VERSION" ]; then
    echo "❌ 错误: 无法从 Cargo.toml 提取版本号。"
    exit 1
fi
echo "📝 当前版本: $VERSION"

# 同步到 tauri.conf.json (使用 sed)
# 寻找 "version": "..." 并替换
sed -i '' "s/\"version\": \"[^\"]*\"/\"version\": \"$VERSION\"/" src-tauri/tauri.conf.json
# 同步到 src-tauri/Cargo.toml
sed -i '' "s/^version = \"[^\"]*\"/version = \"$VERSION\"/" src-tauri/Cargo.toml

# 3. 执行打包
echo "📦 正在运行 Tauri 打包命令..."
npx @tauri-apps/cli build --bundles dmg

# 4. 产物分析
END_TIME=$(date +%s)
DURATION=$((END_TIME - START_TIME))

echo ""
echo "----------------------------------------"
echo "✅ 打包完成！"
echo "⏱️ 总耗时: ${DURATION}s"

DMG_PATH="src-tauri/target/release/bundle/dmg/语音输入法_${VERSION}_aarch64.dmg"
# 注意：文件名可能随架构变化，这里尝试匹配
if [ ! -f "$DMG_PATH" ]; then
    DMG_PATH=$(find src-tauri/target/release/bundle/dmg/ -name "*.dmg" | head -n 1)
fi

if [ -f "$DMG_PATH" ]; then
    SIZE=$(du -h "$DMG_PATH" | cut -f 1)
    echo "📂 DMG 位置: $DMG_PATH"
    echo "⚖️ DMG 大小: $SIZE"
else
    echo "⚠️ 未找到 DMG 文件，请检查 src-tauri/target/release/bundle/dmg/"
fi

echo "📝 应用数据目录: ~/Library/Application Support/语音输入法/"
echo "----------------------------------------"
