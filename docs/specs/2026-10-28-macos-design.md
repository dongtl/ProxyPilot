# ProxyPilot macOS 版设计

日期：2026-10-28

## 背景与约束

- Windows 版 0.2.0 已发布。macOS 版和 Windows 版一起作为 0.3.0 发布，共用一个更新清单。
- 维护者没有 Mac。开发时在 Windows 上做交叉编译检查（`cargo check --target aarch64-apple-darwin`）；运行时的验证靠 GitHub Actions 的 macOS 虚拟机和有 Mac 的测试者。只能在真机上确认的点，统一列在文末“待实测”。
- 不加入 Apple 开发者计划，用临时签名（ad-hoc）。用户第一次打开时要在“系统设置 → 隐私与安全性”里点“仍要打开”。以后改用正式签名和公证，只需改打包配置。

## 目标与非目标

- 功能和 Windows 版一致：自动找代理端口、按网站和按程序分流、最近连接、网络自检、开机自启、一键更新。
- 支持 macOS 12 及以上，Apple 芯片和 Intel 共用一个通用安装包。
- 官方发布的 sing-box 1.14.4 Mac 版构建时没有指定最低系统版本，实际要求 macOS 26（可以从程序文件里的 LC_BUILD_VERSION 看出来）。所以 Mac 版的 sing-box 由我们在 CI 里从源码自己编译：
  - 源码固定在 v1.14.4 对应的提交上；
  - 用和上游相同的 Go 1.26.10，这是最后一个支持 macOS 12 的 Go 版本；
  - 设 `MACOSX_DEPLOYMENT_TARGET=12.0`；
  - 只开我们用到的 `with_clash_api` 构建标签。

  版本、提交、Go 工具链的 SHA256 都记在 `tools/sing-box.lock.json`。
- 不做：Mac App Store、Network Extension（需要付费账号和苹果审核）、公证。

## 总体架构

```
┌──────── ProxyPilot.app（普通用户权限） ────────┐
│ 界面、菜单栏图标、上游探测、健康检查、设置       │
└──────────────┬──────────────────────────────┘
               │ 本机套接字 /var/run/app.proxypilot.helper.sock
               │ 只传结构化的分流设置，不传配置文件
┌──────────────┴──── 后台助手（root，LaunchDaemon） ────┐
│ 校验请求 → 自己生成 sing-box 配置 → 事务式启停          │
│ /Library/PrivilegedHelperTools/app.proxypilot.helper/  │
└──────────────┬────────────────────────────────────┘
               sing-box（root，TUN 网卡 utunN，默认直连）
```

Windows 版保持原样：整个程序以管理员身份运行，自己管理 sing-box。

拆成两个进程的原因有三个：
- WebView 界面不应该以 root 运行。
- 创建 utun 网卡必须是 root。
- 每次改规则都要重启 sing-box，不能每次都弹密码框。

所以把 root 的部分做成一个常驻后台的小程序，界面只给它发指令。

## 后台助手

### 安装、升级、卸载

- **安装：**
  - 用户第一次打开分流开关时，界面用 `osascript … with administrator privileges` 弹出系统密码框，以 root 运行 App 包里的 `proxypilot-helper install --owner <uid>`。
  - 助手把自己和 sing-box 复制到 `/Library/PrivilegedHelperTools/app.proxypilot.helper/`（root:wheel，0755），并清除隔离属性。
  - 然后写入 `/Library/LaunchDaemons/app.proxypilot.helper.plist`，执行 `launchctl bootstrap system`。
  - 运行数据放在 `/Library/Application Support/app.proxypilot.helper/`（root，0700），包括生成的配置、上一份配置和 sing-box 日志。普通用户读不到这些日志。
- **不会自己弹密码框。** 只有用户亲手打开分流开关时才可能弹。开机自启后，如果助手不可用，只在首页提示。
- **升级不弹密码：**
  - 界面连上助手后先握手，比较版本号。版本不同时，让助手从 App 包的 `Resources/helper/` 自我升级。
  - 助手先把 `manifest.json`、它的签名、新的助手和 sing-box 读进自己的 root 目录，再按顺序校验：
    1. 用编译进助手的更新公钥（和 `tauri.conf.json` 里的是同一把）校验 manifest 的 minisign 签名；
    2. 核对两个文件的 SHA256；
    3. 确认 manifest 里的版本不低于当前版本。
  - 全部通过才替换自己，然后退出，由 launchd 重新拉起。任何一项不通过都拒绝升级，界面提示“关掉分流再打开”，走一次带密码的重新安装。
- **卸载：**
  - 设置页有“移除后台助手”按钮，弹密码框后运行已安装的那份助手的 `uninstall`。
  - 它依次停掉 sing-box、执行 `launchctl bootout`、删除 plist、程序目录和数据目录。删除时按文件名逐个删，不递归删目录。
  - 把 App 拖进废纸篓删不掉助手，README 里要写明。

### 通信与权限

- **套接字：** 文件权限是 0666，但每个连接都用 `getpeereid` 取对方的 uid，只接受安装时登记的用户和 root。
- **协议：** 一行一个 JSON，单行上限 1 MB。请求有 `hello`、`apply`、`restart`、`stop`、`status`、`upgrade`。
- **`apply` 只带这些内容：** 上游（类型、IP、端口）、走代理的程序和域名、直连程序、内网域名、DNS 模式、日志级别、Clash API 端口和密钥、上游端口的占用进程名。
  - 助手逐项校验：上游必须是 IP，端口在有效范围内，密钥是十六进制，条目数量和长度都有上限。
  - 配置里的所有路径（日志、工作目录）都由助手自己定。
  - 这样同一用户下的其它程序即使冒充界面，也只能改分流规则，拿不到 root 权限去读写任意文件。
- **启动 sing-box 之前：** 助手检查自己的程序目录、sing-box、数据目录及其各级上级目录。全部要归 root 所有，组和其他人都不能写，否则拒绝启动。
- **生命周期：**
  - 发出 `apply` 的那个连接是“控制连接”。它断开时（界面退出或崩溃），助手停掉 sing-box，网络恢复直连，效果等同 Windows 上的 Job Object。
  - 助手自己被重启时，先结束上次留下的 sing-box：按 pid 文件找到进程，确认它就是自己目录里的那份才结束，不碰别的 sing-box。

### 事务式应用

沿用 `core::CoreManager` 的流程：检查 → 备份 → 停止 → 原子替换 → 启动 → 等待就绪 → 失败回滚。

就绪判断改为“有网卡带着 172.19.0.1 这个地址”。macOS 上 TUN 网卡名只能是 `utunN`，配置里不写网卡名，由 sing-box 自动选号，所以没法按名字找。

## 平台适配（`proxypilot-core::sys::macos`）

| 功能 | Windows | macOS |
|---|---|---|
| 本机监听端口 | GetExtendedTcpTable | `netstat -an -p tcp` 列出所有监听端口，包括以 root 运行的代理核心；`lsof` 补上本用户进程的 pid。查不到 pid 的端口照样探测 |
| 进程列表 | Toolhelp | `proc_listallpids` + `proc_pidpath` 取文件名，失败时退回 `proc_name` |
| 网卡 | GetIfTable2 | `getifaddrs` |
| 系统代理 | 注册表 Internet Settings | `scutil --proxy` |
| 能否开 TUN | 进程令牌是否提权 | 后台助手是否可用 |
| 随机数 | BCryptGenRandom | `getentropy` |
| 开机自启 | 任务计划（最高权限） | `~/Library/LaunchAgents/app.proxypilot.desktop.plist`（RunAtLoad），以普通用户运行，不需要密码 |
| 其它代理工具的 TUN | 按网卡名识别 | 不检测：macOS 上的网卡都叫 utunN，看不出是谁的 |

## 按程序分流

- **按 `.app` 匹配：** macOS 上的程序是 `.app` 包，真正联网的常常是包里的子进程，比如 VS Code 的 “Code Helper”、Chrome 的 “Google Chrome Helper”。
  - 所以用户选的 `.app` 保存成 `Visual Studio Code.app` 这样的名字。
  - 生成配置时转成 `process_path_regex`：`(?i)/Visual Studio Code\.app/Contents/`，包里所有进程都会匹配上。
  - 命令行程序（node、codex 等）仍然按文件名匹配。
- **直连名单：** 同样按 `.app` 处理。内置名单补上 Mac 上常见的代理客户端和企业 VPN，包括它们的 `.app` 名和后台进程名。以 `.exe` 结尾的条目，在 Mac 上去掉 `.exe` 后按进程名匹配，覆盖同名的命令行核心，比如 xray、sing-box、mihomo。
- **最近连接：** 进程路径在某个 `.app` 里面时，显示和“改走代理”加入的都是那个 `.app`，而不是 Helper 子进程。
- **运行中的程序列表：** 按 `.app` 合并，隐藏 `/System`、`/usr/libexec` 等系统程序。

## 界面

- **菜单栏图标：** 左键弹出菜单（Mac 习惯），菜单里有“打开主界面”。主窗口关闭后隐藏 Dock 图标，打开时再显示。Cmd+Q 和菜单里的“退出”效果一样。
- **文案按系统区分：**
  - 添加应用时选 `.app`。
  - 开机自启的说明是“登录后自动在后台运行”。
  - 没装助手时，首页提示“开启分流时会请求一次管理员密码，用来安装后台助手”。
- **设置页：** 新增“后台助手”一栏，显示状态，并提供移除按钮。仅 Mac 显示。

## 打包与发布

- **配置拆分：**
  - `tauri.conf.json` 放公共配置。
  - `tauri.windows.conf.json` 放 NSIS 和 sing-box sidecar。
  - `tauri.macos.conf.json` 放这些：
    - app 和 dmg 两种包
    - 最低系统版本 12.0
    - 临时签名（`signingIdentity: "-"`）
    - `Resources/helper/` 下的四个文件：`proxypilot-helper`、`sing-box`、`manifest.json`、`manifest.json.sig`

  打包工具只给 MacOS 目录里的程序和整个包签名，不会改动 Resources 里的文件，所以 manifest 里记的 SHA256 打包后依然有效。
- **构建步骤：**
  1. sing-box 按 lock 文件从源码编译 arm64 和 amd64 两份，用 lipo 合成一个通用二进制。
  2. 助手分别编译两个架构，同样用 lipo 合成。
  3. 给两者做临时签名，再算 SHA256 写进 manifest，用更新私钥签名。
  4. 最后执行 `tauri build --target universal-apple-darwin`。
- **GitHub Actions：**
  - Windows 和 macOS 两个构建任务各自产出更新清单片段。
  - 建 Release 的任务把它们合并成一个 `latest.json`，里面有 windows-x86_64、darwin-aarch64、darwin-x86_64 三项，Mac 的两项指向同一个通用包。
  - 草稿 Release 里多出 `.dmg` 和 `.app.tar.gz`。
  - Mac 构建失败不阻塞 Windows 发版，草稿里只是少了 Mac 的文件。
- **冒烟测试：** 另有一个手动触发的 macOS 冒烟测试，在 macos-latest 上依次做这些事：
  1. 真的安装助手、开 TUN。
  2. 用本机 SOCKS 验证选中的网站走代理，没选中的直连。
  3. 卸载，确认文件和网卡都清理干净。

## 隐私

规则和 Windows 版相同。助手的日志只在 root 目录里；界面的 `app.log` 在 `~/Library/Application Support/app.proxypilot.desktop/`。

## 待实测

测试者按 `docs/macos-test-checklist.md` 逐项检查：

1. 首次打开（“仍要打开”），安装助手时的密码框，以及“已添加后台项目”通知。
2. Intel 和 Apple 芯片的 Mac 都能打开。
3. 公司 VPN（aTrust、EasyConnect 等）同时运行时，内网是否正常。
4. 一键更新：App 被替换后重新打开，助手自动升级，不弹密码。
5. 开机自启。
6. 系统代理检测，以及和其它代理工具的 TUN 共存。
7. 按程序分流对 Electron 和 Chrome 系的应用是否生效。
