# openNetDrive

[![License: GPL-3.0](https://img.shields.io/badge/License-GPL--3.0-blue.svg)](https://opensource.org/licenses/GPL-3.0)
[![Rust](https://img.shields.io/badge/Rust-1.70+-orange.svg)](https://www.rust-lang.org)
[![Tauri](https://img.shields.io/badge/Tauri-v2-blue.svg)](https://tauri.app)

跨平台的网络驱动器挂载工具，支持通过 WebDAV/SMB 协议将 NAS 共享文件夹映射为本地磁盘。

## 功能特性

- ✅ **WebDAV 协议支持** - 支持 WebDAV/HTTPS 协议连接 NAS
- ✅ **SMB 协议支持** - 支持 SMB/CIFS 协议（Windows 网络共享）
- 🌐 **跨平台** - Windows / macOS / Linux
- 🎨 **现代 UI** - 基于 Tauri v2 + React 的简洁界面
- 🔐 **凭据管理** - 集成系统凭据管理器
- ⚡ **高性能** - Rust 实现，内存安全
- 📋 **自动扫描** - 启动时自动扫描并导入已有的 SMB 连接
- 📝 **详细日志** - 实时显示连接、挂载、断开操作日志

## 平台支持

| 平台 | 状态 | 文件系统 |
|------|------|----------|
| Windows | ✅ 可用 | WinFsp (WebDAV) / net use (SMB) |
| macOS | 📋 计划 | macFUSE |
| Linux | 📋 计划 | FUSE3 |

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
- [WinFsp](https://winfsp.dev/) - WebDAV 挂载需要

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

# 开发模式运行 Tauri 应用
cd packages/tauri-app
npm install
npm run tauri dev
```

### 构建产物位置

**Windows 调试版本**:
```
target/debug/opennetdrive-tauri.exe
target/debug/bundle/nsis/openNetDrive_0.1.0_x64-setup.exe  # 安装包
```

**发布版本**:
```bash
# 构建发布版本（需要配置好 MSYS2 环境）
cargo build --release -p opennetdrive-tauri

# 产物位置
target/release/opennetdrive-tauri.exe
```

### 打包

```bash
cd packages/tauri-app
npm install
npm run tauri build

# 安装包位置：
#   target/debug/bundle/nsis/   (调试版本)
#   target/release/bundle/      (发布版本)
```

## 使用说明

### 添加 SMB 连接
1. 点击"添加连接"按钮
2. 选择协议类型（SMB）
3. 输入服务器地址（如 192.168.1.100）
4. 输入共享名称（如 Public）
5. 选择或自动分配挂载盘符
6. 输入用户名和密码
7. 点击"添加"

### 挂载与断开
- 点击"连接"按钮挂载选中的网络驱动器
- 点击"断开"按钮卸载驱动器
- 连接成功后可点击文件夹图标打开资源管理器

### 自动挂载
- 勾选"启动时自动挂载"，程序启动时自动连接
- 断开后重新打开程序时会自动重新挂载

## 项目结构

```
openNetDrive/
├── Cargo.toml              # 工作空间配置
├── README.md
├── LICENSE
├── packages/
│   ├── core/               # 核心库（协议、配置）
│   │   ├── src/
│   │   │   ├── config.rs   # 配置管理
│   │   │   ├── protocol.rs # 协议 trait
│   │   │   ├── smb.rs     # SMB 协议实现
│   │   │   └── webdav.rs  # WebDAV 协议实现
│   │   └── Cargo.toml
│   ├── mount-win/          # Windows 挂载 (WinFsp)
│   │   ├── src/
│   │   │   ├── driver.rs  # WinFsp 驱动
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── mount-macos/        # macOS 挂载 (macFUSE) - 计划中
│   ├── mount-linux/        # Linux 挂载 (FUSE3) - 计划中
│   ├── cli/                # 命令行工具
│   └── tauri-app/          # Tauri UI 应用
│       ├── src/            # 前端 (React + TypeScript)
│       ├── src-tauri/      # 后端 (Rust)
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