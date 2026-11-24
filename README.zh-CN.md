# AutoSign 自动签名助手

AutoSign 是面向 Windows 和 Apple Silicon Mac 的 iPhone/iPad 临时签名工具。它使用你自己的 Apple ID 为 IPA 签名并安装到设备，支持 USB、同一 Wi-Fi 自动续签，以及从新版 IPA 直接更新已安装 App。

本项目基于 MIT 许可的 [Impactor](https://github.com/claration/Impactor) 开发，并保留上游许可与致谢。

## 主要功能

- Windows 10/11 x64 一键安装包，内置 Apple 移动设备驱动安装流程。
- macOS Apple Silicon DMG。
- 简体中文界面。
- 支持添加多个 Apple ID，并按现有签名记录和账号负载自动分配。
- 手机通过 USB 或已配对的 Wi-Fi 连接后自动续签。
- 查看 Mac 和已连接 iPhone/iPad 上的开发签名 App，并可手动强制续签。
- **直接更新 App**：导入与已安装 App 相同 Bundle ID 的新版 IPA 后，AutoSign 会识别原始 Bundle ID 与签名后 Team ID 的对应关系，沿用原 Apple ID，重新签名并覆盖安装；成功后同时替换续签源文件，后续续签不会退回旧版本。

## 下载

请从 [GitHub Releases](https://github.com/xiaozhangwangxue/iAutoSign/releases) 下载：

- `AutoSign-*-windows-x64-setup.exe`：Windows 安装版。
- `AutoSign-*-windows-x64-portable.exe`：Windows 免安装版。
- `AutoSign-*-macos-arm64.dmg`：Apple Silicon Mac 版。
- `SHA256SUMS.txt`：发布制品校验值。

## 使用

1. 安装并打开 AutoSign，在设置中添加 Apple ID。
2. 连接 iPhone/iPad，并在设备上选择“信任此电脑”。
3. 首次安装：导入或拖入 IPA，在安装页确认后安装。
4. 更新已有 App：保持目标设备已连接，直接导入相同 Bundle ID 的新版 IPA；AutoSign 会自动进入更新流程。
5. 免费 Apple ID 的描述文件通常只有 7 天有效期，建议保持 AutoSign 后台运行以便自动续签。

账号、证书、续签副本和日志只保存在本机用户数据目录：Windows 为 `%APPDATA%\AutoSign`，macOS 为 `~/.config/AutoSign`。请勿公开这些目录。

## 自行构建

Windows x64（Ubuntu/Debian 交叉编译）：

```bash
sudo apt-get install -y mingw-w64 nasm nsis
rustup target add x86_64-pc-windows-gnu
./scripts/build-windows.sh
```

macOS Apple Silicon：

```bash
./scripts/package-macos.sh
```

产物生成在 `dist/`。推送 `v*` 标签时，GitHub Actions 会同时构建 Windows 与 macOS 制品并创建 Release。

## 安全与许可

- Apple ID 凭据用于与 Apple 的签名服务通信，不应提交到 GitHub 或发送给他人。
- macOS DMG 使用 ad-hoc 签名，首次打开可能需要在 Finder 中右键选择“打开”。
- AutoSign 源代码采用 MIT 许可；随安装包分发的 Apple 驱动及其他第三方组件遵循各自许可。
