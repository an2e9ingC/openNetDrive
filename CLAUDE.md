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

### ⚠️ 重要规则

1. **未验证不提交**：用户提出的问题或需求，在没有验证通过之前，禁止提交代码
2. **编译流程**：每次修改代码后，按以下步骤编译：
   - 删除旧的 exe 文件
   - 强制重新编译（touch 源文件 + cargo build --release）
   - 检查 exe 是否成功生成
3. **日志定位**：当出现 bug 或问题时，首先从日志文件定位问题
   - 日志文件位置：`%LOCALAPPDATA%\openNetDrive\logs\opennetdrive_YYYYMMDD.log`
   - 或者 `C:\Users\<用户名>\AppData\Local\openNetDrive\logs\`
   - 日志级别：ERROR > WARN > INFO > DEBUG

### ⚠️ 预发布阶段 - 编译 Release 版本

在预发布验证阶段，修改代码后必须编译出 **release 版本**的 GUI 程序供验证：

```bash
# 编译 release 版本（完整流程）
export PATH="/c/msys64/mingw64/bin:$PATH"
cd packages/tauri-app/src-tauri

# 1. 删除旧的 exe（如果存在）
rm -f ../../../target/release/ond.exe

# 2. 强制重新编译
touch src/main.rs
cargo build --release

# 3. 检查编译结果
ls -lh ../../../target/release/ond.exe
```

**重要：release 版本的所有日志都会显示在 GUI 底部的日志面板中（debug/info/warn/error）**

**注意：可执行文件名称是 `opennetdrive-tauri.exe`，位于 `target/release/` 目录**

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
