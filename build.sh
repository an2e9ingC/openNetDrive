#!/bin/bash

# openNetDrive 编译脚本
# 用法: ./build.sh [debug|release|help]

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color
BOLD='\033[1m'

# 项目根目录
PROJECT_ROOT="$(cd "$(dirname "$0")" && pwd)"
TAURI_DIR="$PROJECT_ROOT/packages/tauri-app/src-tauri"

# 显示帮助
show_help() {
    echo -e "${CYAN}========================================${NC}"
    echo -e "${CYAN}   openNetDrive 编译脚本${NC}"
    echo -e "${CYAN}========================================${NC}"
    echo ""
    echo -e "${BOLD}用法:${NC}"
    echo "    $0 [debug|release|help]"
    echo ""
    echo -e "${BOLD}参数:${NC}"
    echo -e "  ${GREEN}debug${NC}     编译 debug 版本"
    echo -e "  ${GREEN}release${NC}   编译 release 版本"
    echo -e "  ${GREEN}help${NC}      显示帮助信息"
    echo ""
    echo -e "${BOLD}示例:${NC}"
    echo "    $0 debug      # 编译 debug 版本"
    echo "    $0 release   # 编译 release 版本"
    echo ""
    echo -e "${YELLOW}注意: 需要先构建前端 (cd packages/tauri-app && npm run build)${NC}"
    echo ""
    echo -e "${CYAN}输出位置:${NC}"
    echo "    Debug:   $PROJECT_ROOT/target/debug/opennetdrive-tauri.exe"
    echo "    Release: $PROJECT_ROOT/target/release/opennetdrive-tauri.exe"
    echo ""
}

# 检查环境
check_env() {
    # 检查 PATH 中是否有 GCC
    if ! command -v gcc &> /dev/null; then
        export PATH="/c/msys64/mingw64/bin:$PATH"
    fi

    # 再次检查
    if ! command -v gcc &> /dev/null; then
        echo -e "${RED}错误: 未找到 GCC，请确保 MSYS2 GCC 在 PATH 中${NC}"
        exit 1
    fi

    # 检查 Rust
    if ! command -v cargo &> /dev/null; then
        echo -e "${RED}错误: 未找到 Rust，请安装 Rust${NC}"
        exit 1
    fi

    echo -e "${GREEN}✓ 环境检查通过${NC}"
}

# 检查前端是否已构建
check_frontend() {
    if [ ! -d "$PROJECT_ROOT/packages/tauri-app/dist" ]; then
        echo -e "${YELLOW}⚠ 前端未构建，正在构建前端...${NC}"
        cd "$PROJECT_ROOT/packages/tauri-app"
        npm run build
        if [ $? -ne 0 ]; then
            echo -e "${RED}✗ 前端构建失败${NC}"
            exit 1
        fi
        echo -e "${GREEN}✓ 前端构建成功${NC}"
    else
        echo -e "${GREEN}✓ 前端已构建${NC}"
    fi
}

# 编译 debug 版本
build_debug() {
    echo -e "${CYAN}========================================${NC}"
    echo -e "${CYAN}   编译 Debug 版本${NC}"
    echo -e "${CYAN}========================================${NC}"

    check_env

    cd "$TAURI_DIR"

    echo -e "${YELLOW}开始编译...${NC}"

    # 删除旧的 debug exe
    rm -f "$PROJECT_ROOT/target/debug/opennetdrive-tauri.exe"

    # 编译
    cargo build 2>&1

    if [ $? -eq 0 ]; then
        if [ -f "$PROJECT_ROOT/target/debug/opennetdrive-tauri.exe" ]; then
            SIZE=$(ls -lh "$PROJECT_ROOT/target/debug/opennetdrive-tauri.exe" | awk '{print $5}')
            echo ""
            echo -e "${GREEN}========================================${NC}"
            echo -e "${GREEN}   ✓ Debug 版本编译成功!${NC}"
            echo -e "${GREEN}========================================${NC}"
            echo -e "位置: ${BLUE}$PROJECT_ROOT/target/debug/opennetdrive-tauri.exe${NC}"
            echo -e "大小: ${YELLOW}$SIZE${NC}"
            echo ""
        else
            echo -e "${RED}✗ 编译成功但未找到输出文件${NC}"
            exit 1
        fi
    else
        echo -e "${RED}✗ Debug 版本编译失败${NC}"
        exit 1
    fi
}

# 编译 release 版本
build_release() {
    echo -e "${CYAN}========================================${NC}"
    echo -e "${CYAN}   编译 Release 版本${NC}"
    echo -e "${CYAN}========================================${NC}"

    check_env
    check_frontend

    cd "$TAURI_DIR"

    echo -e "${YELLOW}开始编译...${NC}"

    # 删除旧的 release exe
    rm -f "$PROJECT_ROOT/target/release/opennetdrive-tauri.exe"

    # 编译
    cargo build --release 2>&1

    if [ $? -eq 0 ]; then
        if [ -f "$PROJECT_ROOT/target/release/opennetdrive-tauri.exe" ]; then
            SIZE=$(ls -lh "$PROJECT_ROOT/target/release/opennetdrive-tauri.exe" | awk '{print $5}')
            echo ""
            echo -e "${GREEN}========================================${NC}"
            echo -e "${GREEN}   ✓ Release 版本编译成功!${NC}"
            echo -e "${GREEN}========================================${NC}"
            echo -e "位置: ${BLUE}$PROJECT_ROOT/target/release/opennetdrive-tauri.exe${NC}"
            echo -e "大小: ${YELLOW}$SIZE${NC}"
            echo ""
        else
            echo -e "${RED}✗ 编译成功但未找到输出文件${NC}"
            exit 1
        fi
    else
        echo -e "${RED}✗ Release 版本编译失败${NC}"
        exit 1
    fi
}

# 主逻辑
case "$1" in
    debug)
        build_debug
        ;;
    release)
        build_release
        ;;
    help|--help|-h|"")
        show_help
        ;;
    *)
        echo -e "${RED}错误: 未知参数 '$1'${NC}"
        echo ""
        show_help
        exit 1
        ;;
esac