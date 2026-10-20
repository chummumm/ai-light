# 故障排查

## 灯不显示 / 扫描不到

先在Windows蓝牙设置配对且给灯供电。程序扫描已配对兼容HID接口，不是每次都依赖BLE广播。首次没发现或多台时在设备页扫描并选择；手填地址不能含冒号。同时匹配VID/PID/Usage/serial，不对键盘等其他HID输出。退出以前的PowerShell扫描器和其他灯控程序，避免抢读回执。

查看设备页的三个层次：接口是否枚举、Windows输出调用是否成功、是否收到有效协议ACK。第三项未知不等于完全离线，第一项存在也不等于无线链路健康。断电会断线；恢复配对连接后点重新连接。

## 偶尔卡在绿灯，暂停再恢复才正常

先区分软件目标灯态和实体灯态。若总览已显示工作中或空闲，但实体仍绿，可能是 LED 命令未真正生效。0.3.6 及更早版本把「Windows 写入成功但没有灯具回执」也缓存为已发送；后续目标状态不变时不会重发，每 8 秒的 HID 枚举也不能验证实际灯态。

0.3.7 在 Windows 端修复这条路径：缺少有效回执或发送失败时按 5、10、20、30 秒退避重试；收到回执后每 30 秒低频重申当前灯态；新任务、休眠恢复和手动暂停/恢复会重新同步。实际恢复仍取决于蓝牙链路可用和当前硬件调用完成，不能把重试间隔当成实机恢复时限。

重申只发送原有易失 LED 命令，不延长完成倒计时、不补播蜂鸣、不写灯效持久设置。部分固件收到重复呼吸命令可能重新开始当前呼吸周期，需要实机观察其相位变化。升级 Windows 安装包即可获得修复；Ubuntu agent 0.3.5/0.3.6 无需重装 hooks 或清空任务。

若软件本身也显示绿色，查看是否处于「灯效预览」；「测试五分钟熄灯」会临时覆盖普通工作状态，可点「结束预览」。仍需定位时，可在故障尚未恢复前导出诊断，比较 `aggregate.state`、`output`、`preview`、`hardware.api_success` 和 `hardware.acknowledged`。系统写入成功或接口存在都不能单独证明实体灯已经切换。

## 电量未知 / 蜂鸣不响

需要新鲜有效2A19读数；电量≤20、未知、失败或过期均禁鸣。低电量后需两次≥25、间隔至少10秒恢复，不补播旧提醒。先等读取再用声音页试听。音量0、总开关关闭、规则关闭、当前轮已手动停止等也不会响。

此固件raw100不响、raw50最响；不要用外部脚本把寄存器再改成100。UI已做0–50映射。声音设置只在实际播放前按需写入，并读回核对。必须保持目标设备声明蜂鸣能力0x20；无该能力的设备拒绝输出。

电量以固件上报为准，充电时百分比可能跳变；本程序不据此显示充电状态。电压关闭不影响标准电量或低电量保护。

## Ubuntu无法连Windows

先启动Windows程序并检查「Codex接入」监听成功，再运行`light-agent check`。401通常是密钥不匹配，403检查来源IP白名单；连接拒绝/超时检查宿主机IP、端口、防火墙和VM网络。不要公开粘贴client.json。默认HTTP仅适用于可信本地网络。

修改Windows监听IP/端口必须退出重启，重新导出配置；Ubuntu修改client.json后`systemctl --user restart ai-light-relay.service`。

## 手动emit有效，但Codex不变灯

确认安装agent的Linux用户与Codex一致，以及CODEX_HOME一致。Codex重启后在/hooks审阅并信任8项command hooks。检查`codex --version`；不支持相应事件的版本不能靠灯控修复。

### 任务完成后仍一直黄灯呼吸

先看本地状态：

```bash
~/.local/bin/light-agent status
grep -n '^[[:space:]]*notify[[:space:]]*=' "${CODEX_HOME:-$HOME/.codex}/config.toml" || true
```

如果状态仍是 `working`，而你用的是 `codex exec`，这符合 OpenAI Codex 已报告的行为：部分版本只触发 UserPromptSubmit，不触发 Stop/PostToolUse（https://github.com/openai/codex/issues/18607）。升级 AI Light agent 到 0.3.3 后重新执行 `install --client ...`；没有其他 notify 时，config.toml 应出现指向 `light-agent notify` 的根级 notify。

如果已有自己的 notify，安装器会保留它，因此不会自动获得 exec 完成兜底。不要直接覆盖原通知器；在现有 notifier 中把同一 JSON payload 再调用一次 `~/.local/bin/light-agent notify "$PAYLOAD"`。

0.3.3 同时识别当前 Codex 官方隐藏 `thread_title` 临时任务的固定指令，只在内存判断并忽略/清理该 helper 状态，避免其 Working 优先级压住已经完成的主任务。普通用户自己的“生成标题”请求不会因为关键词相似就被过滤。

日志：

```bash
journalctl --user -u ai-light-relay.service -n 80 --no-pager
~/.local/bin/light-agent status
```

清除旧手动error测试会话用`light-agent emit off`。旧Pythonhook指向已删除文件时，审阅并仅删除属于旧灯控的规则。普通回答提问启发式和网络异常覆盖限制见ARCHITECTURE.md。

## 关闭窗口后还在响

X只是隐藏；托盘菜单可停止本次声音或关闭蜂鸣总开关。完全退出用托盘「退出AI Light」。退出与关闭下次登录自启是两件事。

## 无法启动 / 安装包安全提示

检查`%LOCALAPPDATA%\AILight\startup-error.log`，WebView2及Microsoft VC++ x64运行库。日志可能含系统路径，分享前脱敏。未签名预发布包可能被Windows提示；检查源码提交和SHA256SUMS，不关闭安全保护。不要使用第三方DLL下载站。

## Ubuntu交叉编译

- clang-cl缺失：Ubuntu24.04还需`clang-tools`，不能只安装clang。
- AppIndicator panic：装`libayatana-appindicator3-dev`。
- combase.lib缺失：本源码WinRT初始化链接runtimeobject，不复制假库来绕过。
- CRT大量undefined symbol：保留项目`-crt-static`，检查环境RUSTFLAGS是否覆盖它。不要grep整个Cargo registry后把依赖源码里的字符串当配置。
- LNK4099微软PDB缺失仅为warning时，与真正编译失败分开判断。
- 下载失败：检查网络代理和TLS，不关闭证书校验。

## 提交问题

提供操作系统、程序/固件版本、云构建运行链接或首个实际error附近日志、期望与实际行为。不要附访问密钥、卖家安装器、完整私有会话内容。导出的诊断JSON不含token但含设备标识和局域网IP，需再脱敏。
