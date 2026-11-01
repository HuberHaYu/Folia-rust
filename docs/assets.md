# 资源来源

## 应用图标

- `assets/icon.png` 原样复制自 chthollyphile 的 [folia-major](https://github.com/chthollyphile/folia-major) `build/icon.png`（512×512、16 位 RGBA），版权归原作者所有，沿用 AGPL-3.0-only。
- 原版 `package.json` 的 macOS、Windows、Linux 构建均使用这张 PNG；`electron/main.cjs` 的 `APP_ICON_PATH` 也将它传给无边框主窗口。
- `assets/icon.ico` 从上述 PNG 转换为 8 位 RGBA，使用 Pillow 11.3.0 的 LANCZOS 缩小，含 16、32、48、256 像素四档 PNG 帧，保留透明度；转换结果随源码保存，构建期不生成图片。
- 窗口启动时通过已有 `image` 解码 PNG，再交给 iced `window::Settings.icon`；Windows 构建通过 MIT 许可的 `winresource` 0.1.34 嵌入 ICO，不改第三方 crate。

## 与原版的差异

| 原版 | Folia-rust | 范围 |
|---|---|---|
| electron-builder 生成 Linux 桌面入口与 macOS 应用包图标 | 未提供 `.desktop` / `.icns` | 安装包方案待定；本次只接窗口图标与 Windows exe 资源 |
| 托盘图标 | 未接入 | 本次不实现托盘 |
| 主窗口无边框 | 维持无边框，不额外画左上角应用图标 | 系统窗口图标用于任务栏、Alt+Tab 与系统菜单，不新增标题栏 |
