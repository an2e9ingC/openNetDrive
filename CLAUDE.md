# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

openNetDrive 是一个跨平台的网络驱动器挂载工具，支持通过 WebDAV 和 SMB 协议将 NAS 共享文件夹映射为本地磁盘。

## 技术栈

- **后端**: Rust (工作空间结构)
- **前端**: Tauri v2 + React + TypeScript
- **文件系统**: WinFsp (Windows) / macFUSE (macOS) / FUSE3 (Linux)

## 项目结构

```
openNetDrive/
├── Cargo.toml              # 工作空间配置
├── packages/
│   ├── core/               # 核心库 (协议、配置)
│   ├── mount-win/          # Windows 挂载 (WinFsp)
│   ├── mount-macos/        # macOS 挂载 (macFUSE)
│   ├── mount-linux/        # Linux 挂载 (FUSE3)
│   ├── cli/                # 命令行工具
│   └── tauri-app/          # Tauri UI 应用
```

## 常用命令

```bash
# 构建所有包
cargo build

# 开发模式运行 CLI
cargo run -p cli -- help

# 运行 Tauri 应用
cd packages/tauri-app && npm install && npm run tauri dev

# 打包发布
cargo build --release
```

## 开发注意事项

### ⚠️ 环境变量检查 (Windows)

启动开发前，确保 MSYS2 GCC 在 PATH 中：

```bash
gcc --version  # 应该显示 GCC 版本信息
```

如果未找到，添加 `C:\msys64\mingw64\bin` 到系统 PATH，或临时设置：

```bash
export PATH="/c/msys64/mingw64/bin:$PATH"
```

### 其他要求

1. Windows 开发需要安装 WinFsp: https://winfsp.dev/
2. Tauri 开发需要 Rust 最新版和 Node.js 18+
3. 跨平台编译需要各平台的 FUSE 库
