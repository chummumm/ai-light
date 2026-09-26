# 从源码编译

仓库固定 Rust **1.98.1**；优先保留已提交的 `Cargo.lock`。不能通过删除锁文件掩盖构建错误。图标由 Rust `src-tauri/icon_gen.rs` 生成，静态前端无需 npm build。

## 云端（推荐）

[GitHub Actions](https://github.com/chummumm/ai-light/actions/workflows/build.yml) 在 Windows 2022 原生 MSVC 构建桌面应用/NSIS `.exe`，Ubuntu24.04构建agent；测试和两端编译都通过后才允许发布。不是把Linux可执行文件改名为exe。

## Ubuntu 仅编译 agent

安装 rustup 后：

```bash
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev ca-certificates curl
source "$HOME/.cargo/env"
git clone https://github.com/chummumm/ai-light.git
cd ai-light
bash scripts/build-ubuntu.sh
```

输出 `target/release/light-agent`。不要在 Linux 执行 `cargo build --workspace`；图形应用目前仅支持 Windows。

## 全程 Ubuntu：Windows 安装包 + agent

```bash
bash scripts/build-all-ubuntu.sh --install-deps
# 之后无需重复 apt
bash scripts/build-all-ubuntu.sh --windows-only
# 仅裸 exe
bash scripts/build-all-ubuntu.sh --windows-only --exe-only
# 内存较小时
bash scripts/build-all-ubuntu.sh --windows-only --jobs 1
```

脚本需要 LLVM（包括 `clang-tools` 提供的 clang-cl）、NSIS、AppIndicator 开发包。它使用 `cargo-xwin 0.23.1` + MSVC target，而不是MinGW。首次下载 Microsoft SDK/CRT 前请阅读 [cargo-xwin 许可说明](https://github.com/rust-cross/cargo-xwin#readme)。本项目不代替你接受许可。

普通用户运行脚本，只有 apt 用 sudo。无需 Windows VM、Wine、Python、Node。Root构建缓存归root所有，不能假设普通用户共用其工具链。

输出原始位置：

```text
target/x86_64-pc-windows-msvc/release/ai-light.exe
target/x86_64-pc-windows-msvc/release/bundle/nsis/*-setup.exe
target/release/light-agent
```

脚本还将产物复制到 `dist/ubuntu-cross/<本轮编号>/` 并生成校验。日志在 `build-logs/`。保留 `.cargo/config.toml` 的 `-crt-static` 与源码的 `runtimeobject` 链接修复。

## Windows 原生编译

准备 Rust MSVC、Visual Studio Build Tools「使用 C++ 的桌面开发」及 Windows SDK、WebView2 Runtime。参考 [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)。

```powershell
git clone https://github.com/chummumm/ai-light.git
cd ai-light
rustup target add x86_64-pc-windows-msvc
cargo install tauri-cli --version 2.11.5 --locked
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-windows.ps1
```

仅编译应用：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-windows.ps1 -ExeOnly
```

裸 exe 不创建开始菜单和卸载入口；日常使用请选择安装包。首次缺少WebView2时安装包会联网获取引导程序，不是离线包。动态CRT可能需要微软官方x64VisualC++运行库。

## 测试与依赖更新

```bash
cargo test -p light-core -p light-agent --locked
# 可选前端测试，Node22；不属于应用运行依赖
node --test tests/*.test.mjs
node scripts/check-ui.mjs
```

对依赖进行有意更新时先审阅 Cargo.toml/lock diff；工作区版本变化使用 `cargo update --workspace`。云端构建使用相同锁文件和源码提交，发布带源码归档及构建信息。

## 许可证打包

自己分发构建结果时保留 LICENSE，并收集所含第三方组件的许可证。CI 使用 `node scripts/collect-licenses.mjs <target>` 收集Cargo包中的LICENSE/COPYING/NOTICE等文本，Windows安装器一起携带，Ubuntu归档也携带。请对新增或异常许可证单独审阅；自动收集器不是法律审查工具。
