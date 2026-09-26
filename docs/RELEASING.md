# 云构建与发布（维护者）

## 自动构建

main提交和Pull Request运行测试及Windows/Ubuntu构建，Actions的Artifacts提供`windows-x64`、`ubuntu-agent`和测试日志，保留30天。两端使用prepare输出的同一源码SHA，依赖用已提交Cargo.lock固定。Action引用完整commit SHA，Rust固定1.98.1、Tauri CLI2.11.5。

首次仓库初始化时如果main缺Cargo.lock，prepare可以取已通过测试的bootstrap锁文件并提交；之后始终`--locked`。这一步用GITHUB_TOKEN写入不会无限触发自己的push流程。正常开发不应删除锁文件。若分支保护拒绝首次提交，应由维护者把锁文件正常提交，而不是关闭保护。

## 创建新预发布版

1. 将根Cargo.toml的workspace version、src-tauri/tauri.conf.json版本一起增加，更新UI静态版本标记和变更日志。
2. 本地`cargo update --workspace`并提交Cargo.lock。
3. 推送main后，可在Actions → Build and release → Run workflow勾选publish；也可以提交信息带`[release]`，或推送匹配版本的`vX.Y.Z`标签。
4. 只有prepare、Windows、Ubuntu都成功后，release job才用contents:write创建draft、上传全部产物，然后转为公开pre-release。

已公开同名版本不会自动覆盖。修复后发布新版本，或明确处理失败留下的draft后再重试。不要把不同提交的文件塞入已发布版本。

## 发行文件

- Windows x64 NSIS安装exe（创建开始菜单与卸载入口）。
- Ubuntu24.04 x86_64 agent归档，含使用文档、LICENSE和依赖许可证。
- 同一源码提交的tar.gz，含Cargo.lock。
- Cargo.lock、依赖清单/许可证包、build-info.txt和SHA256SUMS。

Windows使用原生Windows-2022/MSVC，不把跨编译模拟测试当作原生验收。CI不连接用户硬件，也不验证听感/电池精度；发布说明明确是未签名预发布版。没有配置自动更新和代码签名证书，不要把签名私钥放入仓库。

## CI权限与安全

默认contents:read。prepare仅在受信任main首次锁文件提交时写入；PR不持久化checkout凭据，fork token仍受GitHub读权限限制。release只在受信任分支/标签且非PR运行并使用contents:write。不要改用pull_request_target来执行未知PR源码；不要在日志输出token/client.json。

## 发布前实机检查

安装/卸载、开始菜单、登录自启静默、单实例、X隐藏、托盘退出；四种灯态与旧计时；低电量禁鸣/恢复；蜂鸣模式和音量读回；设备断线重连、Windows睡眠恢复、来源IP变化与防火墙。CI绿色只证明已执行的构建和测试，不代替这些硬件检查。
