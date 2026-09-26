# 兼容设备协议摘要

这是针对已测试`5E 5E` BLE HID设备的实现说明，不是所有名为AI Light的设备通用规范。不同硬件/固件应先确认兼容性，不发送未定义命令。

## 标识与封包

VID `07D7`，PID `6B01`，Usage Page `FF00`，Usage `0001`。Windows HID报告为Report ID `02` +64字节payload，不足补零。

```text
5E 5E LENGTH CODE DATA... FCS
```

LENGTH计数CODE、DATA、FCS；FCS为前面字节逐字节XOR，因此包含FCS的整帧XOR为0。匹配帧长、校验和、命令及目标地址后才接受响应。

## 已使用命令

| 命令 | 用途 | 限制 |
|---|---|---|
| `02` | READ寄存器 | 运行时只允许05/11/14，不暴力扫描 |
| `03` | WRITE设置 | 仅音量寄存器05，原始值限制0–50 |
| `04` | 易失LED效果 | 红黄绿、关闭、呼吸；不写灯效持久设置 |
| `05` | 蜂鸣模式 | 0停止、1短、2双、3三、4长 |

LED通道1绿、2黄、4红、7全部；action0关闭、1常亮、2闪烁。已使用TLV：01 FF排他显示，02 00持续，03周期（100ms单位），04渐变（100ms单位）。

```text
绿色常亮：5E 5E 06 04 01 01 01 FF FC
全部熄灭：5E 5E 04 04 07 00 07
短鸣：    5E 5E 03 05 01 07
停止声音：5E 5E 03 05 00 06
音量raw50：5E 5E 04 03 05 32 30
```

音量不是可直接映射声压的百分比。已测试固件在raw50附近最响，raw100无声，符合占空比型控制表现；程序只用0–50并将其标为相对档位，不把PWM机理推断当成已取得固件源码。

## 电池、能力与版本

| 地址 | 含义/证据 |
|---|---|
| 05 | 兼容客户端的buzzer.volume设置，原始允许0–100；本程序进一步限制0–50 |
| 10 | 设备类型/模式选择；不解码为充电状态 |
| 11 | 能力位，0x20表示客户端允许蜂鸣，0x01用于RGB功能判断 |
| 12 | 客户端HID电量百分比；BLE时客户端使用标准2A19，本程序亦如此 |
| 13 | 三个字节major.minor.patch版本；本程序不作握手写入 |
| 14 | little-endian u16毫伏，兼容客户端用2000–5000校验 |

READ成功为`82 address payload...`，错误为`42 error`。标准BLE电量使用Battery Service180F / Characteristic2A19的一个0–100字节。当前程序没有实现直接充电状态，因为目标设备未观察到可验证状态字段；不根据电压或百分比冒充正在充电。

## 两种Windows输出路径

对已测试设备，LED与READ使用`HidD_SetOutputReport`；蜂鸣及音量写入使用hidapi的Windows`WriteFile`输出路径。保留这个实测差异，不宣称任意适配器或固件有同样行为。只有API成功不等于设备已执行；读取/写入回执、音量读回和实际观察分别记录。

## 依据与边界

帧、传统LED和READ格式参考公开逆向项目：https://github.com/DiGuStudent/promlight-linux/blob/main/PROTOCOL.md 。扩展寄存器、蜂鸣和电压解析通过兼容客户端1.0.33的静态分析和用户设备实测核对；不附带或反编译分发卖家应用。公开旧文档只列1–4，不能因此把其余地址认定无效。

微软HID资料：https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/sending-hid-reports 。标准电池服务：https://www.bluetooth.com/specifications/specs/battery-service/ 。不要将其他GB_TRANS/55AA协议或OTA固件套用到这里。
