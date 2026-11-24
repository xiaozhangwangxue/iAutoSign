# AutoSign

[简体中文说明](README.zh-CN.md)

AutoSign is an open-source iOS sideloading and renewal tool for Windows x64 and Apple Silicon Macs. It signs IPA files with the user's own Apple ID, installs them over USB or a paired Wi-Fi connection, and keeps managed apps renewable.

## Highlights

- Windows setup, portable executable, and Apple Silicon macOS DMG.
- Multiple Apple IDs with deterministic account reuse and load balancing.
- USB/Wi-Fi renewal, manual forced renewal, and installed signed-app discovery.
- Direct IPA updates: importing a newer IPA with the same original bundle ID reuses the previous Apple ID, re-signs and overwrites the installed app, then replaces the stored renewal source so later renewals keep the new version.
- Simplified Chinese and English UI.

Download verified artifacts from [GitHub Releases](https://github.com/xiaozhangwangxue/iAutoSign/releases). Build instructions, usage, storage locations, and security notes are in the [Chinese README](README.zh-CN.md).

AutoSign is based on the MIT-licensed [Impactor](https://github.com/claration/Impactor). Source code is MIT licensed; bundled third-party components remain under their respective licenses.
