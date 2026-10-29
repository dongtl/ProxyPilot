# ProxyPilot 代理分流助手

只让你选中的应用和网站走代理，其余流量照常直连。公司内网、国内网站不再被代理拖慢。

ProxyPilot 不提供节点，它把流量交给你已经在用的代理客户端（v2rayN、Clash 等）。它不读取、也不修改代理客户端的配置和订阅。

> 当前是 0.3 内测版，支持 Windows 10/11；Mac 版（macOS 12 及以上）为测试版。

## 支持哪些代理工具

只要代理工具在本机开了 SOCKS5 或 HTTP 端口，并且端口没有设置用户名密码，就能用。主流客户端基本都满足这个条件。

- 能自动识别：v2rayN、Clash Verge Rev、FlClash、Mihomo / Clash Meta、NekoBox、Hiddify、sing-box、Shadowsocks 等。
- 没被识别的客户端：点“全部扫描”，或者手动填写端口。
- 局域网里其他设备上的代理（软路由、另一台电脑）：手动填写 IP 和端口。
- 只开了 HTTP 端口的：需要手动填写。HTTP 端口转发不了 UDP，语音通话、游戏可能受影响。

暂时用不了：

- 只有 VPN 模式、不开本地端口的工具，比如快连、ExpressVPN、NordVPN、WireGuard、OpenVPN。
- 设置了用户名密码的代理端口。

## 安装

1. 到 [Releases 页面](https://github.com/dongtl/ProxyPilot/releases/latest) 下载 `ProxyPilot_版本号_x64-setup.exe`，双击安装。
2. 如果出现蓝色的“Windows 已保护你的电脑”，点“更多信息”，再点“仍要运行”。这是因为安装包还没有做代码签名。
3. 每次打开 ProxyPilot 都会弹出“你要允许此应用对你的设备进行更改吗”，选“是”。创建虚拟网卡需要管理员权限。在设置里打开“开机自动启动”后，开机时不会再弹。

可以装在任意位置，包括 D 盘。安装程序会把安装目录设成只有管理员能修改，防止别的程序替换掉以管理员身份运行的文件。

卸载：Windows 设置 → 应用 → ProxyPilot → 卸载。卸载时会一并删除开机自启任务。

### Mac（测试版，0.3 起）

需要 macOS 12 或更新的版本，Apple 芯片和 Intel 都能用。

1. 下载 `ProxyPilot_版本号_universal.dmg`，双击，把 ProxyPilot 拖进“应用程序”。忘了拖也没关系：直接打开时它会提示，点“移过去”就会自己搬进“应用程序”并重新打开。
2. 第一次打开会提示“无法验证开发者”：点“完成”，然后打开 系统设置 → 隐私与安全性，在“安全性”里点“仍要打开”，输入开机密码。这是因为安装包没有经过苹果公证，只需要这一次。
3. 第一次打开分流开关时，系统会请求开机密码，用来安装一个后台助手（创建虚拟网卡需要管理员权限）。以后开关分流、更新版本都不用再输入。

如果提示“已损坏，无法打开”，在“终端”里执行 `xattr -dr com.apple.quarantine /Applications/ProxyPilot.app` 后再打开。

卸载：先在设置页关掉“开机自动启动”，再点“移除后台助手”，最后退出 ProxyPilot，把它拖进废纸篓。直接拖进废纸篓删不掉后台助手。

Mac 版还在测试，帮忙测试的朋友可以按 [测试清单](docs/macos-test-checklist.md) 逐项检查。

## 更新

- 从 0.2.0 开始，ProxyPilot 每天检查一次新版本。有新版本时界面顶部会出现提示，点“立即更新”会自动下载、校验并安装，装完自动重新打开，分流中断几秒钟。
- 安装包带有签名，校验不通过的不会安装。
- 在“设置 → 软件更新”里可以手动检查，也可以关掉自动检查。
- 0.1.0 没有这个功能，需要手动升级一次：下载新版安装包直接安装。提示已安装旧版本时选默认选项；如果弹出卸载窗口，不要勾选“删除应用数据”，设置就会保留。

## 使用

1. 在代理客户端里：
   - 关闭“系统代理”；
   - 关闭 TUN / 虚拟网卡模式；
   - 用“全局”或“规则”模式，不要用“直连”模式。
2. 打开 ProxyPilot，它会自动查找本机的代理端口，选一个点“使用这个”。
3. 到“分流项目”里打开要走代理的服务（ChatGPT、Claude、Google、GitHub 等），也可以添加自己的网站和程序。
4. 回到首页，打开分流总开关。

关闭窗口不会退出，ProxyPilot 会留在右下角托盘里继续工作。要完全退出，右键托盘图标点“退出”，网络会恢复成没装本工具时的样子。Mac 上图标在屏幕顶部的菜单栏，点它再点“退出”。

“最近连接”页能看到每个连接走的是代理还是直连，可以筛选和搜索，也能在那里把某个网站、整个域名或程序直接改成走代理。想仔细看的时候点“暂停刷新”，列表就不再变动；“清空”可以从头开始观察。

## 遇到问题

| 现象 | 处理 |
|---|---|
| 分流开了，被墙的网站还是打不开 | 检查代理客户端是不是在“直连”模式；在首页点“测试代理”，确认代理本身能用 |
| 提示“系统代理已开启” | 在代理客户端里关掉系统代理，否则浏览器会绕过分流 |
| 提示“其它代理工具的虚拟网卡已开启” | 在代理客户端里关掉 TUN / 虚拟网卡模式，两个虚拟网卡会互相抢路由 |
| 找不到代理端口，或提示“代理客户端未运行” | 打开代理客户端。FlClash 等客户端打开后还要在首页点“启动”才会开端口，在它的设置里打开“自动运行”可以省掉这一步。如果它换了端口，在首页点“更换”重新选择 |
| 提示“ProxyPilot 虚拟网卡已被占用” | 一般是旧版 ProxyPilot 还在运行，从托盘退出它即可，分流会自动重试 |
| 提示“安装目录权限不安全” | 用最新的安装包重新安装一次（装回原来的位置也可以），安装程序会自动修正权限 |
| 公司内网网址打不开 | 在“设置 → 公司内网域名”里添加公司的域名（比如 corp.example.com）；还不行就把 DNS 模式改成“系统 DNS” |
| 公司 VPN 连上后时好时坏 | 把 VPN 程序加到“设置 → 永远直连的程序”。深信服、Cisco、FortiClient 等常见企业 VPN 已经自动包含 |

还是解决不了的话，在首页点“网络自检”，再点“复制报告”，把报告发给开发者。

## 隐私

- 不读取代理客户端的配置、订阅和节点，没有任何数据上报。
- 检查更新时会访问 GitHub 读取最新版本信息，不发送任何个人数据。代理客户端在运行时会通过它访问。可以在设置里关掉自动检查。
- 网络自检报告里不包含节点信息、IP 地址和内网地址，可以放心发给别人。
- 默认日志只记录问题，不记录访问了哪些网站。
- “最近连接”只保存在内存里，最多保留最近 300 条，关闭 ProxyPilot 后清空。
- 程序自己的日志 `app.log` 记录启动、分流开关、“测试代理”的结果（本机端口、测试网站和耗时）和检查更新的结果，同样不含节点信息和访问记录。
- 设置和日志保存在本机的 `%LOCALAPPDATA%\app.proxypilot.desktop`，在设置页可以一键打开。sing-box 的配置和日志放在安装目录下的 `runtime` 文件夹，只有管理员能修改。
- Mac 上设置和日志在 `~/Library/Application Support/app.proxypilot.desktop`，sing-box 的配置和日志在 `/Library/Application Support/app.proxypilot.helper`，只有管理员能读写。

## 项目现状

0.3 内测版，主要功能已经完成：

- 自动找到代理端口，代理客户端换端口后会自动跟上
- 按应用或网站分流，内置 ChatGPT、Claude、Google、YouTube、GitHub、Cursor、开发者软件源等常用项目
- 企业 VPN、企业微信、钉钉和代理客户端自身始终直连
- 环境检查、网络自检、托盘、开机自动启动、安装包
- 应用内一键更新
- Mac 版（测试版）

已知限制：

- 安装包没有代码签名，安装时会有安全提示
- Windows 版每次手动打开都要确认管理员权限
- 不支持设置了用户名密码的代理端口
- 分流效果还在实机验证中，目前只用 v2rayN 测试过，其它客户端欢迎反馈
- Mac 版还没有在真机上完整测过

后续计划：代码签名、Windows 后台服务模式（打开时不再需要管理员确认）、规则包。

## 开发

需要：Rust（stable，≥ 1.90）、Node.js ≥ 20、pnpm、Visual Studio C++ 生成工具、WebView2。

```powershell
pnpm install
powershell -ExecutionPolicy Bypass -File scripts/fetch-sing-box.ps1   # 下载并校验固定版本的 sing-box
cargo test --workspace                                                 # Rust 单元测试和 sing-box 配置检查
pnpm build                                                             # 前端类型检查和构建
pnpm tauri dev                                                         # 开发模式运行
pnpm tauri build                                                       # 打包安装程序（不签名），输出在 target/release/bundle/nsis/
```

正式版推送版本标签后由 GitHub Actions 签名打包，并生成应用内更新用的 `latest.json`，步骤见 [CONTRIBUTING.md](CONTRIBUTING.md)。各版本的更新内容见 [CHANGELOG.md](CHANGELOG.md)。

开发构建不会请求管理员权限。要真正开启分流（创建虚拟网卡），请在“以管理员身份运行”的终端里执行 `pnpm tauri dev`。正式版启动时会请求管理员权限。

`proxypilot-cli` 可以在不启动界面的情况下探测端口、检查环境、生成配置：

```powershell
cargo run -p proxypilot-core --bin proxypilot-cli -- detect
cargo run -p proxypilot-core --bin proxypilot-cli -- env --upstream socks5://127.0.0.1:10808
```

## 目录

- `crates/proxypilot-core`：分流逻辑（配置生成、端口探测、健康检查、环境检查、网络自检、核心进程管理），不依赖界面。
- `src-tauri`：Tauri 外壳（窗口、托盘、后台任务、IPC）。
- `src`：Vue 3 前端。
- `docs/specs`：设计文档。
- `scripts`：下载 sing-box、发版、Phase 0 实机验证脚本（开发者用）。

## 工作原理

ProxyPilot 启动一个 [sing-box](https://github.com/SagerNet/sing-box) 虚拟网卡，默认所有流量直连，只把你选中的程序和域名转发到代理客户端的本地端口。企业 VPN 和代理客户端自身的流量始终直连，不会被套进虚拟网卡。详见 [设计文档](docs/specs/2026-10-27-universal-split-routing-design.md)。

## 许可证

ProxyPilot 以 [GPL-3.0](LICENSE) 发布。随附的第三方组件见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。ProxyPilot 是独立项目，不是 sing-box 项目的一部分，也未获得其作者的认可或背书。

参与贡献前请阅读 [CONTRIBUTING.md](CONTRIBUTING.md)。
