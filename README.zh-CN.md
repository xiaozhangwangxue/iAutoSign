# AutoSign 自动签名助手（Windows）

AutoSign 是一个 Windows 端的 iPhone 临时签名工具：用你自己的 Apple ID 给 IPA
签名并直接安装到手机上，并在手机连接电脑或与电脑处于同一个 Wi-Fi 时自动续签
（免开发者账号，使用 Apple 的免费个人团队证书）。

本项目基于 MIT 许可的开源项目 [Impactor](https://github.com/claration/Impactor)
（Rust，原作者 claration / khcrysalis）构建，并针对 Windows 新电脑安装体验、
Wi-Fi 自动续签与若干已知问题做了修改。

## 和 Sideloadly 相比做了什么

* **安装即用（离线也算）**：安装包内置了 Apple 官方的驱动组件
  （`AppleMobileDeviceSupport64.msi`），安装时直接本地装好——不需要联网、
  不需要 winget、不需要微软商店，也不用先装 iTunes；Bonjour 缺失时会尽力补装。
* **本地 anisette**：优先在本地生成 Apple 的设备身份数据（omnisette + Apple
  Music 的 Android 库），不再依赖第三方 anisette 服务器；服务器不可用时才会回退。
* **自动续签**：
  * 手机通过数据线连接时会立刻检查是否需要续签；
  * 插线时会自动为设备打开“Wi-Fi 连接”，之后手机只要和电脑在同一网络即可续签；
  * 可在设置里选择“提前几天续签”（默认 7 天证书提前 3 天）。
* **开机自启**：安装时可勾选，后台常驻托盘，到点自动续签。
* **中文界面**：安装向导与应用界面均为简体中文。

## 使用步骤

1. 运行 `AutoSign-*-windows-x64-setup.exe`，一路下一步（保持“Apple 手机驱动与
   依赖”勾选，首次安装会联网下载 Apple 官方驱动）。
2. 打开 AutoSign → 设置 → 添加账号，输入 Apple ID 与密码，按提示输入双重认证
   验证码（建议使用一个只用于签名的 Apple ID）。
3. 用数据线连接 iPhone，在手机上点“信任此电脑”。
4. 回到主界面，把要安装的 `.ipa` 拖进窗口，选择设备后点击安装。
5. 安装完成后可以在设置里看到自动续签状态；手机插线或与电脑同一 Wi-Fi 时会
   自动完成续签（应用需要保持运行，可在安装时勾选开机自启）。

> 免费 Apple ID 签发的证书只有 7 天有效期，这是 Apple 的限制；自动续签就是
> 在到期前重新签名并安装一次，让应用不会失效。

## 自己编译

```bash
# Ubuntu/Debian
sudo apt-get install -y mingw-w64 nasm nsis
rustup target add x86_64-pc-windows-gnu

./scripts/build-windows.sh
```

产物在 `dist/`：

* `AutoSign-<版本>-windows-x64-setup.exe` —— 一键安装包（含驱动安装逻辑）
* `AutoSign-<版本>-windows-x64-portable.exe` —— 免安装版
* `AutoSign-<版本>-windows-x64-cli.exe` —— 命令行签名工具 `plumesign`

## 许可

MIT。上游 Impactor 的版权与许可声明保留在 `LICENSE` 中。
