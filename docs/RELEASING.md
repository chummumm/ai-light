# 云构建与发布（维护者）

## 自动构建

包含源码、依赖、构建配置或运行资源变更的main提交和Pull Request运行测试及Windows/Ubuntu构建，Actions的Artifacts提供`windows-x64`、`ubuntu-agent`和测试日志，保留30天。两端使用prepare输出的同一源码SHA，依赖用已提交Cargo.lock固定。Action引用完整commit SHA，Rust固定1.98.1、Tauri CLI2.11.5。

仅修改Markdown、MDX、reStructuredText、AsciiDoc、`docs/**`或GitHub issue/PR说明模板时，push和PR不启动自动构建或发布，包括提交信息带`[release]`的纯文档提交。文档配图归入`docs/**`；`ui/**`、`src-tauri/**`等实际运行资源、配置、依赖和构建脚本仍触发构建。文档与这些文件混合修改时照常执行。显式手动构建和版本标签不受文档路径过滤影响，保留原有发布条件。

Cargo.lock已在初始化时提交。每次构建使用`--locked`，不会自动升级或修改依赖。需要有意更新依赖时由维护者审阅并提交锁文件。常规prepare和PR构建只需contents:read，不自动推送代码。

## 创建新预发布版

1. 将根Cargo.toml的workspace version、src-tauri/tauri.conf.json版本一起增加，更新UI静态版本标记和变更日志。
2. 本地`cargo update --workspace`并提交Cargo.lock。
3. 推送main后，可在Actions → Build and release → Run workflow勾选publish；也可以提交信息带`[release]`，或推送匹配版本的`vX.Y.Z`标签。
4. 只有prepare、Windows、Ubuntu都成功后，release job才用contents:write创建draft、上传全部产物，然后转为公开pre-release。

已公开同名版本不会自动覆盖。修复后发布新版本，或明确处理失败留下的draft后再重试。不要把不同提交的文件塞入已发布版本。已有同名标签指向不同源码时拒绝发布。

## 正式版（0.3.6 起）

需要直接发布正式版时，在main的发布提交信息中同时写入`[release] [stable]`。工作流仍先运行全部测试和两端构建，上传完产物后才将本次新版本设为非预发布并标记Latest。普通`[release]`、标签发布和未显式选择正式渠道的手动构建仍走预发布；不要在未经授权的提交上添加正式发布标记。

已有预发布版也可以通过Actions → Promote stable release手动选择版本提升；提升过程不替换原有二进制产物。正式发布标签不代表程序已代码签名或硬件已经全面实测。

可在`docs/releases/vX.Y.Z.md`写入该版本专用的更新和升级说明，发布脚本会将其附加到Release正文。

## 发行文件

- Windows x64 NSIS安装exe（创建开始菜单与卸载入口）。
- Ubuntu24.04 x86_64 agent归档，含使用文档、LICENSE和依赖许可证。
- 同一源码提交的tar.gz，含Cargo.lock。
- Cargo.lock、依赖清单/许可证包、build-info.txt和SHA256SUMS。

Windows使用原生Windows-2022/MSVC，不把跨编译模拟测试当作原生验收。CI不连接用户硬件，也不验证听感/电池精度；发布说明保留未签名提示。没有配置自动更新和代码签名证书，不要把签名私钥放入仓库。

## CI权限与安全

默认contents:read，checkout不持久化凭据。release仅在受信任分支/标签且非PR运行，单独授予contents:write。不要改用pull_request_target来执行未知PR源码；不要在日志输出token/client.json。Windows调用node.exe直接运行Tauri CLI，保留Cargo的`-- --locked`参数分隔，不经过会消费`--`的PowerShell脚本shim。

## 发布前实机检查

安装/卸载、开始菜单、登录自启静默、单实例、X隐藏、托盘退出；四种灯态与旧计时；低电量禁鸣/恢复；蜂鸣模式和音量读回；设备断线重连、Windows睡眠恢复、来源IP变化与防火墙。CI绿色只证明已执行的构建和测试，不代替这些硬件检查。
