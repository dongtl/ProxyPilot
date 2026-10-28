# 第三方组件声明

ProxyPilot 安装包随附以下第三方程序。它们作为独立进程运行，ProxyPilot 只通过配置文件和命令行与之交互。

## sing-box

- 版本：1.14.4（以 `tools/sing-box.lock.json` 为准）
- 作者：nekohasekai 及贡献者
- 许可证：GNU General Public License v3.0 or later，附加条款：衍生作品未经许可不得使用其名称或暗示与其存在关联。许可证全文见安装目录 `licenses/sing-box-LICENSE.txt`。
- 对应源码：https://github.com/SagerNet/sing-box/tree/v1.14.4
- Windows 版随附的是上游发布的未修改二进制文件：https://github.com/SagerNet/sing-box/releases/tag/v1.14.4
- Mac 版随附的是用上述源码（提交 c005f35300ce76427dc285481ec5695f5b179cd2）未经修改编译的二进制文件。上游发布的 Mac 版要求 macOS 26，所以改为自行编译：最低系统设为 macOS 12，只开启 `with_clash_api` 构建标签。编译方法见 `scripts/build-macos-helper.sh`，参数固定在 `tools/sing-box.lock.json`。

ProxyPilot 不是 sing-box 项目的一部分，也未获得其作者的认可或背书。

sing-box 在 Windows 上内置了 WireGuard 项目的 Wintun 驱动，用于创建虚拟网卡，其许可证见 https://www.wintun.net/ 。

## 界面依赖

前端依赖（Vue、Naive UI、Tauri 等）的许可证随各自的包发布，可在 `pnpm-lock.yaml` 和 `Cargo.lock` 中查到具体版本。
