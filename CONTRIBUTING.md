# 参与贡献

欢迎提交问题和改进。为了让项目以后能继续自由选择授权方式（包括另行提供商业授权），所有代码和数据贡献都需要同意下面的贡献者许可协议。

## 贡献者许可协议（CLA）

提交 Pull Request / Merge Request 即表示你同意以下条款：

1. 你是所提交内容的作者，或者有权提交它；提交内容不侵犯任何第三方的权利。
2. 你保留所提交内容的著作权。
3. 你授予项目维护者一项永久、全球、免费、不可撤销、非独占的许可，允许其以任何方式使用、复制、修改、分发所提交的内容，并允许其以 GPL-3.0 以外的任何条款（包括商业授权）再授权。
4. 项目维护者可以选择不接受任何提交。

本协议同样适用于预设、规则、VPN 目录等数据贡献。

> 维护者注：正式对外接受贡献前，建议请律师审阅本协议文本。

## 提交前

- `cargo test --workspace` 和 `pnpm build` 都要通过。
- 修改分流规则或 sing-box 配置时，同步更新 `docs/specs` 里的设计文档。
- 日志和诊断信息里不能出现节点地址、UUID、密码、订阅链接、WS 路径、Host 或请求内容。
- 预设只收录明确的服务域名，不收录范围过大的后缀（例如整个 `amazonaws.com`）。

## 发版（维护者）

正式版由 GitHub Actions 打包签名（`.github/workflows/release.yml`）：

1. 把 `src-tauri/tauri.conf.json`、`package.json`、`Cargo.toml` 里的版本号改成同一个新版本，在 `CHANGELOG.md` 最上面写这个版本的更新内容。这段内容会显示在应用的更新提示里。
2. 提交并推送，然后推送版本标签：`git tag v版本号`，`git push origin v版本号`。
3. 等 Actions 跑完（约 20 分钟），Releases 里会出现一个草稿，带着安装包和 `latest.json`。下载安装包装一遍，确认没问题再点发布，不要勾选 Pre-release。

想先试试能不能打包，可以在 Actions 页面手动运行 Release，这样只上传文件，不建 Release。

签名私钥和密码存在仓库的 Actions 密钥里：`TAURI_SIGNING_PRIVATE_KEY` 是 `proxypilot.key` 文件的全部内容，`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 是密码。公钥在 `tauri.conf.json` 的 `plugins.updater.pubkey` 里。不要重新生成钥匙，已安装的版本只认这一把；私钥不要放进仓库，并且要备份，私钥丢了，已安装的用户就再也收不到自动更新。

Actions 用不了时，可以在本机运行 `powershell -ExecutionPolicy Bypass -File scripts/release.ps1 -Key 私钥路径`（`.pub` 要放在同一个文件夹），按提示输入密码，再手动新建 Release，上传安装包和 `latest.json`。
