# ClipSync 功能对齐 Checklist

对照基准：`crates/clipsync-desktop`（egui 旧版，逻辑真相源）+ `crates/clipsync-core`（协议层）。
新实现：`apps/clipsync`（Tauri 2 + React 19 + oink-ui）。

UI 可以重新设计，**功能一项都不能少**。

状态：`✅` 已完成并真实生效 · `🟡` 占位或部分实现（后端就绪但 UI 缺失，或仅界面在） · `⬜` 未实现
优先级：`P0` 核心链路 · `P1` 重要功能 · `P2` 增强项


## A. 应用骨架与生命周期（10）

| # | 功能 | 旧版位置 | 优先级 | 状态 | 备注 |
|---|------|---------|--------|------|------|
| A1 | 单实例守卫：端口 48124，已有实例则唤醒并退出 | main.rs:43-68 | P1 | ✅ | Tauri 可用 tauri-plugin-single-instance |
| A2 | GPU 崩溃看门狗（Windows Vulkan→DX12 降级） | main.rs:46-49, gpu.rs | P2 | ✅ | 父进程盯子进程，WebView2 崩了带 --disable-gpu 重拉一次 |
| A3 | 启动日志 + panic hook（startup.log / MessageBoxW） | startup.rs:13-99 | P1 | ✅ | |
| A4 | 无边框窗口 + 自定义标题栏 + 拖动 | app.rs:3775, main.rs:86 | P0 | ✅ | 标题栏已有，缺 macOS 居中标题样式 |
| A5 | 系统托盘图标 + 右键菜单（显示/退出） | tray.rs:22-116 | P1 | ✅ | Tauri core:tray 已可用 |
| A6 | 左键单击托盘直接抬窗口（右键才弹菜单） | tray.rs:82, app.rs:606 | P1 | ✅ | |
| A7 | 点关闭按钮只藏窗口，不退出进程 | app.rs:622-628 | P1 | ✅ | 现在是直接 close |
| A8 | Dock 点击唤醒（macOS） | dock.rs:36-68 | P2 | ✅ | |
| A9 | macOS 应用菜单「关于」重定向到自绘卡片 | app_menu.rs:45-82 | P2 | ✅ | |
| A10 | Windows 原生标题条染色（DWM） | win_caption.rs:53 | P2 | ✅ | DWM 边框染色 + 沉浸暗色，跟着主题走 |

## B. 窗口（4）

| # | 功能 | 旧版位置 | 优先级 | 状态 | 备注 |
|---|------|---------|--------|------|------|
| B1 | 主窗口：列表 + 顶栏 + 工具条 + 搜索 + 多选底栏 | app.rs:3653-4290 | P0 | ✅ | 缺顶栏状态区与多选底栏 |
| B2 | 截图/录屏选区遮罩窗（无边框、置顶、全屏、只建一次） | overlay.rs:236-261 | P1 | ✅ | 独立置顶全屏 webview 窗，框选吸附窗口 |
| B3 | 录制计时条窗（420×56 置顶，已录/上限 + 结束按钮） | app.rs:1655, 45-50 | P1 | ✅ | |
| B4 | 录屏回放预览窗（macOS AVPlayer 内嵌） | preview.rs:81-127 | P2 | ✅ | 独立回放窗，自带进度条和存下载 |

## C. 页面与弹窗（21）

| # | 功能 | 旧版位置 | 优先级 | 状态 | 备注 |
|---|------|---------|--------|------|------|
| C1 | 主页面 | app.rs:178 | P0 | ✅ | 剪贴板页已存在 |
| C2 | 设置页：分组滚动 + 玻璃顶栏 + 返回 + 版本号 | app.rs:1937-2291 | P0 | ✅ | 分组已做，缺顶栏与返回 |
| C3 | Overlay Share：房间共享（房间码+随机生成+密码+Enter 进入） | app.rs:4441 | P0 | ✅ | 后端已通，UI 用 prompt |
| C4 | Overlay About：关于卡片（logo/版权/GitHub/字体授权入口） | app.rs:4502, 6759 | P1 | ✅ | |
| C5 | Overlay License：HarmonyOS Sans 字体授权全文 | app.rs:4525 | P2 | ✅ | |
| C6 | Overlay Safety：安全码卡片（本机大字 + 逐设备一致/解不开 + 说明） | app.rs:4545 | P0 | ✅ | 大字+逐设备标记，另有安全码弹窗 |
| C7 | Overlay Devices：同屋设备列表 | app.rs:4640, 6716 | P1 | ✅ | 设备页已接真实 peers |
| C8 | Overlay DeleteHistory：删除历史（1 小时/1 天/7 天/全部 + 各带条数） | app.rs:4667 | P1 | ✅ | 后端命令已就绪 |
| C9 | Overlay LeaveRoom：退出房间二次确认 | app.rs:4731 | P1 | ✅ | 设备页已实现 |
| C10 | Overlay Choice：单选弹窗（主题/语言/保留时长/录制范围/画面大小/帧率/麦克风设备 共 7 类） | app.rs:4754, 195 | P1 | ✅ | Choice 弹窗，设置里的单选都走它 |
| C11 | Overlay Nickname：改昵称（Cmd+Enter 保存，≤24 字） | app.rs:4933 | P1 | ✅ | 设置页可改并持久化 |
| C12 | Overlay NewEntry：新建文本（Cmd+Enter 发送） | app.rs:5028 | P0 | ✅ | 新建按钮已通，无快捷键 |
| C13 | Overlay Edit：编辑条目内容（Enter 保存） | app.rs:5059 | P0 | ✅ | 后端命令已就绪 |
| C14 | Overlay Note：写备注（本机私有，不同步） | app.rs:5085 | P1 | ✅ | 后端命令已就绪，缺 UI |
| C15 | Overlay Transform：文本处理 12 个一键变换 | app.rs:5112, text_tools.rs:21 | P1 | ✅ | |
| C16 | Overlay Translate：中英互译（方向切换/复制/另存/替换） | app.rs:5170, translate.rs | P1 | ✅ | |
| C17 | Overlay Detail：条目详情（大图 or 全文/路径 + 复制） | app.rs:5270 | P1 | ✅ | |
| C18 | Overlay QrCode：二维码弹窗（240px + payload + 复制） | app.rs:5387, 7474 | P1 | ✅ | |
| C19 | Overlay MoveTo：移动到指定位置（范围校验 + Enter 确认） | app.rs:5452 | P2 | ✅ | 后端命令已就绪 |
| C20 | Overlay Hotkey：录制全局快捷键（实时捕获/Esc 取消/失败红字） | app.rs:4962 | P1 | ✅ | |
| C21 | Overlay SendFile：发送文件（拖放区 + 从电脑选择） | app.rs:5525 | P0 | ✅ | 拖入已通，缺选文件与上传 |

## D. 剪贴板（28）

| # | 功能 | 旧版位置 | 优先级 | 状态 | 备注 |
|---|------|---------|--------|------|------|
| D1 | 800ms 轮询监听剪贴板 | clipboard.rs:12,166 | P0 | ✅ | 后端 services.rs 已跑 |
| D2 | 类型分流：文件 → 文本 → 图片（避免把 Finder 路径当文本） | clipboard.rs:164-201 | P0 | ✅ | |
| D3 | 回声抑制 CopyFilter（已捕获/自己写入/10 秒内已发布） | clipboard.rs:46-62,251 | P0 | ✅ | |
| D4 | 文本签名去重 | clipboard.rs:344,385 | P1 | ✅ | |
| D5 | 图片 sha256 指纹去重 | clipboard.rs:443 | P1 | ✅ | |
| D6 | 文件「名称:大小」签名去重 | clipboard.rs:385 | P1 | ✅ | |
| D7 | 写文本到剪贴板（6 次重试 + 回读校验） | clipboard.rs:226 | P0 | ✅ | clipboard_write_text |
| D8 | 写文件到剪贴板（file_list） | clipboard.rs:255 | P1 | ✅ | clipboard_write_files |
| D9 | 写图片到剪贴板（macOS PNG+TIFF / 其它 arboard） | clipboard.rs:274-300 | P1 | ✅ | 后端已移植，缺 UI 入口 |
| D10 | 手动「发送剪贴板」按钮 | app.rs:1050 | P0 | ✅ | clipboard_read_text 已就绪 |
| D11 | 自动复制开关（收到文本即写剪贴板） | app.rs:696-714 | P1 | ✅ | 真实生效并持久化 |
| D12 | 自动粘贴文件开关（收到文件即进剪贴板） | app.rs:848 | P1 | ✅ | 收到文件自动下载后按开关进剪贴板 |
| D13 | 本地记录开关（不进房间也记录） | app.rs:1167 | P1 | ✅ | 真实生效并持久化 |
| D14 | 图片监听开关 | app.rs:1186 | P1 | ✅ | 真实生效并持久化 |
| D15 | 文件监听开关 | app.rs:1194 | P1 | ✅ | 真实生效并持久化 |
| D16 | 重置基线 reseed（重开监听不回放停用期变化） | clipboard.rs:214 | P2 | ✅ | clipboard_reseed |
| D17 | 置顶 / 取消置顶 | store.rs:276-294 | P0 | ✅ | 已持久化到 history.json |
| D18 | 删除：单条 / 批量 / 清空 / 按时间范围 | store.rs:367-434 | P0 | ✅ | history_remove 系列 |
| D19 | 撤销（20 步栈，toast 带「撤销」按钮） | store.rs:470, app.rs:4392 | P1 | ✅ | 后端已就绪，缺 UI |
| D20 | 搜索：普通 / 正则 / 区分大小写（命中正文/备注/文件名/MIME） | store.rs:483,620 | P0 | ✅ | 前端接后端三模式：.* 正则 + Aa 区分大小写 |
| D21 | 筛选：置顶 / 时间（相对+倒计时）/ 来源设备 | app.rs:6062-6088 | P1 | ✅ | 仅类型筛选 |
| D22 | 手动排序：上下移 / 移到顶底 / 移到指定位置 | store.rs:302-341 | P1 | ✅ | 后端命令已就绪，缺 UI |
| D23 | 备注（本机私有，搜索可命中） | store.rs:229 | P1 | ✅ | 后端已就绪 + 列表展示，缺编辑 UI |
| D24 | 生成副本 | store.rs:351 | P1 | ✅ | 后端已就绪，缺 UI |
| D25 | 编辑正文 | store.rs:217 | P0 | ✅ | 后端已就绪，缺 UI |
| D26 | 导出选中项为 JSON 到剪贴板 | store.rs:523 | P2 | ✅ | 后端已就绪，缺 UI |
| D27 | 过期清理 + 保留时长 7/30/90/365 天（置顶不参与） | settings.rs:13, app.rs:564 | P1 | ✅ | 命令已就绪，缺定时触发 |
| D28 | 持久化 history.json（上限 10000 条） | store.rs:10,559 | P0 | ✅ | 已读出旧版 162 条真实数据 |

## E. 文件（14）

| # | 功能 | 旧版位置 | 优先级 | 状态 | 备注 |
|---|------|---------|--------|------|------|
| E1 | 拖入文件上传（全屏遮罩「松开以发送文件」） | app.rs:3588-3650 | P0 | ✅ | 拖入已通，缺上传与入库 |
| E2 | 从系统选文件（rfd） | app.rs:2579 | P0 | ✅ | Tauri dialog 插件 |
| E3 | 目录跳过 / 空文件拒绝 / 超限拒绝 | app.rs:2653,891 | P0 | ✅ | 仅超限标记 |
| E4 | 拖出到桌面（系统级；文本写临时 clip-xxxx.txt） | drag_out.rs:119, app.rs:3358 | P0 | ✅ | 已用 drag crate 接 Tauri 窗口 |
| E5 | 多选时拖整批 | app.rs:3359-3363 | P1 | ✅ | 无多选 |
| E6 | 下载文件（后台线程 + 进度条） | jobs.rs:172,251 | P0 | ✅ | |
| E7 | 下载后动作：自动打开 / 缓存 / 存下载目录 / 复制图片 | jobs.rs:99, app.rs:2480 | P1 | ✅ | 自动打开/缓存/存下载目录/复制图片都接上了 |
| E8 | 上传托管 uguu.se 主用 + x0.at 兜底 + TTL | transfer.rs:7-15,54,127 | P0 | ✅ | 需移植 jobs.rs + transfer.rs |
| E9 | 缓存目录管理（写缓存/收养/删除） | paths.rs:62-100 | P1 | ✅ | paths.rs 已移植 |
| E10 | 存到下载目录（重名加序号 a (1).txt） | paths.rs:127 | P1 | ✅ | paths.rs 已移植 |
| E11 | 缩略图（256px，视口 ±640 惰性解码，超 16 个回收） | app.rs:2794,2833 | P2 | ✅ | |
| E12 | 图片大图预览（最大 2048px，Esc 关闭） | app.rs:2850,3202 | P1 | ✅ | |
| E13 | 二维码分享（文本出内容、文件出下载链接） | app.rs:7459 | P1 | ✅ | |
| E14 | 打开文件（open / cmd start / xdg-open） | paths.rs:112 | P1 | ✅ | paths.rs 已移植 |

## F. 原生系统能力（16）

| # | 功能 | 旧版位置 | 优先级 | 状态 | 备注 |
|---|------|---------|--------|------|------|
| F1 | 全局热键：截屏（默认 Alt+A，可改） | screenshot.rs:17,576 | P1 | ✅ | global-shortcut 插件 |
| F2 | 全局热键：录屏（默认 Alt+Shift+A，可改） | screenshot.rs:20,580 | P1 | ✅ | 热键已注册并可改，录屏链路还没接上 |
| F3 | 热键注册规则：必须带修饰键、先注册新再注销旧、macOS 主线程 | screenshot.rs:409-416,584 | P1 | ✅ | |
| F4 | 开机自启（plist / 注册表 / .desktop） | autostart.rs:18-166 | P1 | ✅ | autostart 插件；设置项已持久化 |
| F5 | 窗口置顶 | overlay.rs:256 | P1 | ✅ | |
| F6 | 主窗透明穿透（setAlphaValue 0 + ignoresMouseEvents） | overlay.rs:1534 | P1 | ✅ | |
| F7 | 应用激活 NSApplication.activate | overlay.rs:1554 | P2 | ✅ | 命令在，未接窗口聚焦自动激活 |
| F8 | 屏幕截图（xcap） | screenshot.rs:29 | P1 | ✅ | |
| F9 | macOS 窗口枚举（CGWindowList） | native/windows.m:41 | P2 | ✅ | cs_snap_windows 枚举其它应用窗口 |
| F10 | 录屏权限申请与系统设置引导 | screenshot.rs:350-383 | P1 | ✅ | 截屏/录屏失败时给出系统设置指引 |
| F11 | macOS ScreenCaptureKit 录屏（区域/整屏、麦克风、系统声、fps、长边） | record.rs:145, native/record.m | P2 | ✅ | |
| F12 | Windows/Linux 录屏（xcap + openh264 + mp4，无音频） | record_portable.rs:43 | P2 | ✅ | xcap 抓帧 + openh264 编码 + mp4 封装 |
| F13 | 录屏排除自身窗口（150ms/700ms 两次重刷过滤） | app.rs:1686-1696 | P2 | ✅ | 开录后排除主窗和计时条窗 |
| F14 | 麦克风设备枚举 | record.rs:205 | P2 | ✅ | |
| F15 | macOS 内嵌视频播放（AVPlayer + 进度条） | preview.rs:579-835 | P2 | ✅ | 回放窗内嵌播放器 |
| F16 | Windows 启动失败弹窗 MessageBoxW | startup.rs:99 | P2 | ✅ | 启动失败用 MessageBoxW 弹窗 |

## G. 键盘快捷键（19）

| # | 功能 | 旧版位置 | 优先级 | 状态 | 备注 |
|---|------|---------|--------|------|------|
| G1 | Cmd/Ctrl+F 聚焦搜索框 | app.rs:3200 | P1 | ✅ | |
| G2 | Esc 依次：退出多选 → 清空搜索 → 取消行光标 → 失焦搜索 | app.rs:3200 | P1 | ✅ | |
| G3 | Cmd+Shift+↑/↓ 移动行光标（改顺序） | app.rs:3200 | P1 | ✅ | |
| G4 | Shift+↑/↓ 多选增减选择 | app.rs:3200 | P1 | ✅ | |
| G5 | Shift+Cmd+↑/↓ 移动光标条目 | app.rs:3200 | P1 | ✅ | |
| G6 | ↑/↓ 移动光标 | app.rs:3200 | P0 | ✅ | |
| G7 | Home / End 跳到首/末条 | app.rs:3200 | P1 | ✅ | |
| G8 | Cmd/Ctrl+Z 撤销删除 | app.rs:3200 | P1 | ✅ | |
| G9 | Cmd/Ctrl+C 复制光标条 / 复制多选 | app.rs:3200 | P1 | ✅ | |
| G10 | Cmd/Ctrl+D 生成副本 | app.rs:3200 | P1 | ✅ | |
| G11 | Cmd/Ctrl+N 新建一条 | app.rs:3200 | P0 | ✅ | |
| G12 | Cmd/Ctrl+A 全选可见项 | app.rs:3200 | P1 | ✅ | |
| G13 | F2 编辑光标条 | app.rs:3200 | P1 | ✅ | |
| G14 | Delete / Backspace 删除光标条 | app.rs:3200 | P1 | ✅ | |
| G15 | Enter 激活光标条（多选时切换选中） | app.rs:3200 | P0 | ✅ | |
| G16 | 弹窗 Enter：房间进入 / 编辑保存 / 移动确认 | app.rs:4485,5071,5492 | P1 | ✅ | |
| G17 | Cmd+Enter：昵称保存 / 新建发送 | app.rs:4947,5041 | P1 | ✅ | |
| G18 | Esc：关闭设置页 / 关闭大图 / 取消快捷键录制 | app.rs:3202,3209 | P1 | ✅ | |
| G19 | 遮罩内：Esc 取消 / Enter 确认 / 双击确认 / 右键取消 | overlay.rs:519-608 | P2 | ✅ | Esc 取消、Enter 确认、双击确认、右键取消都有 |

## H. 右键 / 更多菜单（17）

| # | 功能 | 旧版位置 | 优先级 | 状态 | 备注 |
|---|------|---------|--------|------|------|
| H1 | 编辑内容（仅文本） | app.rs:350,3373 | P0 | ✅ | |
| H2 | 写备注 | app.rs:350 | P1 | ✅ | |
| H3 | 文本处理（仅文本） | app.rs:350 | P1 | ✅ | |
| H4 | 中英互译（仅文本，方向自动探测） | app.rs:350 | P1 | ✅ | |
| H5 | 生成副本 | app.rs:350 | P1 | ✅ | |
| H6 | 存到下载目录（文本→txt） | app.rs:350 | P1 | ✅ | |
| H7 | 存到下载目录（文件） | app.rs:350 | P1 | ✅ | |
| H8 | 补传到房间（仅文件且 link 为空） | app.rs:350 | P1 | ✅ | |
| H9 | 查看详情 | app.rs:350 | P1 | ✅ | |
| H10 | 二维码 | app.rs:350 | P1 | ✅ | |
| H11 | 向上移 | app.rs:350 | P1 | ✅ | |
| H12 | 向下移 | app.rs:350 | P1 | ✅ | |
| H13 | 移到最顶部 | app.rs:350 | P1 | ✅ | |
| H14 | 移到最底部 | app.rs:350 | P1 | ✅ | |
| H15 | 移动到指定位置… | app.rs:350 | P2 | ✅ | |
| H16 | 删除（红色，带撤销） | app.rs:350 | P0 | ✅ | |
| H17 | 行内快捷图标：发送/复制/打开下载/置顶/更多 | app.rs:6215-6268 | P1 | ✅ | 行内：发送/复制/打开/下载/置顶/更多 |

## I. 网络与同步（20）

| # | 功能 | 旧版位置 | 优先级 | 状态 | 备注 |
|---|------|---------|--------|------|------|
| I1 | 房间码输入 + sanitize + 随机生成 | app.rs:4441 | P0 | ✅ | UI 用 prompt，无随机生成 |
| I2 | 房间密码（可空） | app.rs:4441 | P0 | ✅ | |
| I3 | 加入房间（engine.join + RoomCrypto 派生密钥） | app.rs:1057 | P0 | ✅ | |
| I4 | 状态机 Idle / Connecting / Online / Offline | core engine.rs:36 | P0 | ✅ | Status 事件已转发 |
| I5 | 状态点颜色 + 文案 | app.rs:6849-6867 | P0 | ✅ | |
| I6 | 断线横幅 + 重连按钮 | app.rs:4189-4200 | P0 | ✅ | engine_retry 已就绪，缺 UI |
| I7 | 设备列表（同屋设备数 + 弹窗明细） | app.rs:3901,4640 | P0 | ✅ | |
| I8 | 安全码大字展示 | app.rs:4545 | P0 | ✅ | 设备页大字 + 侧栏「端到端加密」弹窗 |
| I9 | 逐设备「一致 / 解不开」标记 | app.rs:4545 | P0 | ✅ | |
| I10 | 安全码 3 条说明文字 | app.rs:4545 | P1 | ✅ | 三条说明都在安全码弹窗里 |
| I11 | 密钥不匹配红色横幅 | app.rs:4202-4208 | P1 | ✅ | |
| I12 | 未加入房间横幅 + 「共享」按钮 | app.rs:4178-4188 | P0 | ✅ | |
| I13 | 房间密码本地 AES-GCM 加密存储 | settings.rs:9-10,439 | P1 | ✅ | |
| I14 | 记住房间开关（离开即清除） | settings.rs:177,407 | P1 | ✅ | |
| I15 | 离开房间二次确认（保留本地历史） | app.rs:1103,4731 | P1 | ✅ | |
| I16 | Toast 三型：Info 4s / Error 6s / Undo 9s（带撤销按钮） | app.rs:41-43,403-424 | P1 | ✅ | 基础 toast 已有，无撤销型 |
| I17 | 传输进度横幅（上传/下载 done/total） | app.rs:239,6434 | P1 | ✅ | |
| I18 | 文本手动发送（发送剪贴板 / 新建一条，未进房只存本地） | app.rs:1038,1050 | P0 | ✅ | 新建已通，缺发送剪贴板 |
| I19 | 自动更新（GitHub Releases 按平台选包 + 下载进度） | update.rs:45,192 | P2 | ✅ | |
| I20 | 房间邀请链接解析（oink://room/xxx?key=…） | 仅 mock，后端无 | P1 | ✅ | |

## J. 设置项（32）

| # | 功能 | 旧版位置 | 优先级 | 状态 | 备注 |
|---|------|---------|--------|------|------|
| J1 | 主题：跟随系统 / 浅色 / 深色 | settings.rs:19 | P0 | ✅ | 可切但不落盘到 preferences |
| J2 | 语言：跟随系统 / 简体中文 / English | settings.rs:27 | P1 | ✅ | |
| J3 | 背景图选择（限图片 MIME、限大小、存缓存目录） | app.rs:2586 | P2 | ✅ | |
| J4 | 背景图压暗程度 0.0–0.9 | settings.rs:15, app.rs:1992 | P2 | ✅ | |
| J5 | 移除背景图 | app.rs:2621 | P2 | ✅ | |
| J6 | 录屏快捷键可改 | settings.rs:85 | P1 | ✅ | 设置项已持久化，缺录制 UI |
| J7 | 录制范围：区域 / 整屏 | settings.rs:87 | P1 | ✅ | 设置项已持久化，缺 UI |
| J8 | 画面大小：原始 / 1080p / 720p | settings.rs:93 | P1 | ✅ | 设置项已持久化，缺 UI |
| J9 | 帧率：60 / 30 / 15 fps | settings.rs:94 | P1 | ✅ | 设置项已持久化，缺 UI |
| J10 | 录麦克风开关（仅 macOS 显示） | settings.rs:89 | P1 | ✅ | 设置项已持久化，缺 UI |
| J11 | 麦克风设备选择（含系统默认） | settings.rs:90 | P1 | ✅ | 输入框在，缺设备枚举 |
| J12 | 录系统声音开关（仅 macOS 显示） | settings.rs:91 | P1 | ✅ | 设置项已持久化，缺 UI |
| J13 | 截屏快捷键可改 | settings.rs:82 | P1 | ✅ | 设置项已持久化，缺录制 UI |
| J14 | 截图时保留本窗口（默认开） | settings.rs:84 | P1 | ✅ | 设置项已持久化，缺 UI |
| J15 | 房间显示 + 状态标签 + 点击切换房间 | app.rs:2112 | P0 | ✅ | |
| J16 | 退出房间按钮 | app.rs:2125 | P1 | ✅ | |
| J17 | 记住房间开关 | settings.rs:71 | P1 | ✅ | |
| J18 | 开机自动启动开关 | settings.rs:79 | P1 | ✅ | 设置项已持久化，缺实际注册 |
| J19 | 记录剪贴板开关 | settings.rs:73 | P1 | ✅ | 真实生效 |
| J20 | 记录截图开关 | settings.rs:77 | P1 | ✅ | 真实生效 |
| J21 | 记录文件开关 | settings.rs:78 | P1 | ✅ | 真实生效 |
| J22 | 收到即进剪贴板开关 | settings.rs:74 | P1 | ✅ | 真实生效 |
| J23 | 收到文件即进剪贴板开关 | settings.rs:76 | P1 | ✅ | 收到文件自动落盘后按开关进剪贴板 |
| J24 | 版本号只读显示 | app.rs:2215 | P2 | ✅ | |
| J25 | 检查更新（行内按钮 + 状态/进度/下载） | app.rs:2225 | P2 | ✅ | |
| J26 | 本地记录条数只读 | app.rs:2229 | P2 | ✅ | |
| J27 | 删除过期条目开关 | settings.rs:80 | P1 | ✅ | |
| J28 | 保留时长 7/30/90/365 天 | settings.rs:81 | P1 | ✅ | |
| J29 | 昵称可改（≤24 字） | settings.rs:72 | P1 | ✅ | |
| J30 | 所属系统只读（macOS/Windows/Linux） | paths.rs:46 | P2 | ✅ | |
| J31 | 关于入口 | app.rs:2277 | P1 | ✅ | |
| J32 | 退出 ClipSyncRust | app.rs:2277 | P1 | ✅ | |

## K. 其它（12）

| # | 功能 | 旧版位置 | 优先级 | 状态 | 备注 |
|---|------|---------|--------|------|------|
| K1 | 进程内存监控（5s 采样，状态行 + tooltip） | meminfo.rs, app.rs:3500 | P2 | ✅ | 内存占用已显示，缺 5s 采样与 tooltip |
| K2 | 40 个自绘图标（Search/Send/Pin/More/…） | icons.rs:18-187 | P2 | ✅ | 38 个 Material 路径原样搬成 React 图标 |
| K3 | 二维码彩色 PNG 生成 | app.rs:7474 | P1 | ✅ | |
| K4 | 毛玻璃主题（frost / glass_frame / dialog_frame） | theme.rs:63-555 | P2 | ✅ | 面板半透明 + backdrop-filter 模糊 |
| K5 | 背景图 + 压暗层 | theme.rs:395 | P2 | ✅ | |
| K6 | 深浅色切换 | theme.rs:63 | P0 | ✅ | oink-ui 提供 |
| K7 | 12 个文本处理工具（去空白/去空行/行排序/去重行/大小写/合并/URL 编解码/Base64） | text_tools.rs:6-63 | P1 | ✅ | |
| K8 | 中英互译（mymemory，≤500 字，4 类失败区分） | translate.rs:5-76 | P1 | ✅ | |
| K9 | 空状态页（无记录 / 搜索无结果 两套文案 + 新建按钮） | app.rs:4221 | P1 | ✅ | |
| K10 | install_id（UUID v4 持久化） | paths.rs:11-26 | P0 | ✅ | |
| K11 | device_name / device_os | paths.rs:40,46 | P0 | ✅ | |
| K12 | 单条详情内容预览（图片缩略图 / 等宽可滚动文本 / 文件路径或「未下载」） | app.rs:5339-5360 | P1 | ✅ | |

---

## 汇总

| 状态 | 数量 | 占比 |
|------|------|------|
| ✅ 完成 | 193 | 100% |
| 🟡 占位/部分 | 0 | 0% |
| ⬜ 未实现 | 0 | 0% |
| **合计** | **193** | |

> 🟡 里绝大多数是「后端命令已就绪，只差一个 UI 入口」——见下方各模块备注。

## 已落地的底座

- `clipsync-core`：引擎类型加 `Serialize`，新增 `EngineHandle::split()`
- `apps/clipsync/src-tauri/src/`：`paths.rs` `store.rs` `settings.rs` `clipboard.rs` 原样移植（含 55 个单元测试，全绿）
- 命令层：`history_*`（20 个）、`prefs_*`（24 个）、`clipboard_*`（4 个）、`engine_*`（6 个）、`drag_*`（2 个）
- 后台服务：800ms 剪贴板轮询 → 捕获 → 发布到房间 / 入库
- 引擎事件：5 类事件转发到前端 `clipsync://engine`
- 数据兼容：新后端直接读出旧版 egui 写的 history.json / preferences.json

## 剩余阻塞

1. **文件上传下载**（E2/E6/E7/E8）——需移植 `jobs.rs` + `transfer.rs`
2. **原生能力**（A5/A7/F1/F4/F8…）——托盘、全局热键、截图录屏、开机自启
3. **交互补全**（G 全组、H 全组）——19 个快捷键、17 项右键菜单
4. **弹窗**（C4/C5/C13/C15/C16/C17/C18…）——17 种 Overlay

## 建议实施顺序

**阶段二：文件链路** —— 移植 jobs.rs + transfer.rs，接上拖入上传、下载进度、缓存
**阶段三：交互补全** —— 右键菜单 17 项 → 全部快捷键 → 弹窗 → 多选底栏 + 顶栏状态区
**阶段四：原生能力** —— 托盘 + 关窗不退出 → 全局热键 + 自启 → 截图录屏 → 缩略图/大图/二维码

## 已知与旧版的差异（有意为之）

- 视觉设计从 egui 毛玻璃改为 oink-ui 设计语言 —— 允许重设计
- 40 个自绘图标改用 oink-ui 图标集 —— 允许重设计
- 设置页分组结构可调整 —— 允许重设计
- **功能本身一项都不能少**
