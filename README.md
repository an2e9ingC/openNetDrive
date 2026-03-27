# openNetDrive

[![License: GPL-3.0](https://img.shields.io/badge/License-GPL--3.0-blue.svg)](https://opensource.org/licenses/GPL-3.0)
[![Rust](https://img.shields.io/badge/Rust-1.70+-orange.svg)](https://www.rust-lang.org)
[![Tauri](https://img.shields.io/badge/Tauri-v2-blue.svg)](https://tauri.app)

跨平台的网络驱动器挂载工具，支持通过 WebDAV/SMB 协议将 NAS 共享文件夹映射为本地磁盘。

## 功能特性

- ✅ **WebDAV 协议支持** - 支持 WebDAV/HTTPS 协议连接 NAS
- 🔄 **SMB 协议支持** (开发中) - 支持 SMB/CIFS 协议
- 🌐 **跨平台** - Windows / macOS / Linux
- 🎨 **现代 UI** - 基于 Tauri v2 的简洁界面
- 🔐 **凭据管理** - 集成系统凭据管理器
- ⚡ **高性能** - Rust 实现，内存安全

## 平台支持

| 平台 | 状态 | 文件系统 |
|------|------|----------|
| Windows | 🚧 开发中 | WinFsp |
| macOS | 📋 计划 | macFUSE |
| Linux | 📋 计划 | FUSE3 |

## 开发计划

### 阶段一：Windows + WebDAV 原型
- [x] 项目初始化
- [x] WebDAV 协议实现
- [x] WinFsp 文件系统驱动框架
- [x] 命令行配置工具
- [x] Tauri UI 基础框架

### 阶段二：Tauri UI
- [x] 连接管理界面
- [x] 自动挂载功能
- [ ] 系统托盘
- [ ] 设置对话框
- [ ] 状态监控面板

### 阶段三：跨平台支持
- [ ] macOS (macFUSE) 支持
- [ ] Linux (FUSE3) 支持

### 阶段四：SMB 协议
- [ ] SMB 协议完整实现
- [ ] 双协议支持

## 快速开始

### 环境要求

#### Windows
- Rust 1.70+
- Node.js 18+
- MSYS2 MinGW-w64 工具链
  ```bash
  # 下载安装 MSYS2: https://www.msys2.org/
  # 安装 mingw-w64 工具链
  pacman -S mingw-w64-x86_64-toolchain
  ```
- 确保 `/mingw64/bin` 在 PATH 中，或设置环境变量：
  ```bash
  PATH=/c/msys64/mingw64/bin:${PATH}
  ```
- [WinFsp](https://winfsp.dev/) - 运行时（开发时可选）

#### macOS
- Rust 1.70+
- Node.js 18+
- Xcode Command Line Tools
- [macFUSE](https://osxfuse.github.io/)

#### Linux
- Rust 1.70+
- Node.js 18+
- `fuse3` 包
  ```bash
  # Debian/Ubuntu
  sudo apt install fuse3 libfuse3-dev
  # Arch Linux
  sudo pacman -S fuse3
  ```

### 构建

```bash
# 克隆仓库
git clone https://github.com/an2e9ingC/openNetDrive.git
cd openNetDrive

# 设置环境变量（Windows）
export PATH="/c/msys64/mingw64/bin:${PATH}"

# 构建所有包
cargo build

# 开发模式运行 CLI
cargo run -p opennetdrive-cli -- help

# 开发模式运行 Tauri 应用
cd packages/tauri-app
npm run tauri dev
```

### 构建产物位置

**Windows 调试版本**:
```
packages/tauri-app/src-tauri/target/debug/opennetdrive-tauri.exe  # Tauri 应用可执行文件
```

**发布版本**:
```bash
# 构建发布版本
cargo build --release

# 产物位置
target/release/opennetdrive-tauri.exe    # Tauri 应用
target/release/opennetdrive-cli.exe      # 命令行工具
```

### 打包

```bash
# CLI
cargo build --release -p cli

# Tauri 应用（生成安装包）
cd packages/tauri-app
npm install
npm run tauri build

# 安装包位置：packages/tauri-app/src-tauri/target/release/bundle/
#   - .msi 安装包 (Windows)
#   - .exe 独立安装程序
#   - .app (macOS)
#   - .deb / .rpm (Linux)
```

## 项目结构

```
openNetDrive/
├── Cargo.toml              # 工作空间配置
├── README.md
├── LICENSE
├── packages/
│   ├── core/               # 核心库
│   │   ├── src/
│   │   │   ├── config.rs   # 配置管理
│   │   │   ├── protocol.rs # 协议 trait
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── mount-win/          # Windows 挂载
│   │   ├── src/
│   │   │   ├── driver.rs   # WinFsp 驱动
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── cli/                # 命令行工具
│   │   ├── src/
│   │   │   └── main.rs
│   │   └── Cargo.toml
│   └── tauri-app/          # Tauri UI
│       ├── src/            # Rust 后端
│       ├── src-tauri/      # Tauri 配置
│       └── package.json    # 前端依赖
```

## 开源协议

本项目采用 **GPL-3.0** 协议开源。

- 允许商业使用、修改和分发
- 修改版本必须以相同协议开源
- 商业授权请联系作者

## 参与贡献

欢迎提交 Issue 和 Pull Request！

## 致谢

- [WinFsp](https://github.com/winfsp/winfsp) - Windows 文件系统驱动框架
- [Tauri](https://github.com/tauri-apps/tauri) - 跨平台桌面应用框架
- [RaiDrive](https://raidrive.com/) - 灵感来源

---

**openNetDrive** - 让云存储触手可及
