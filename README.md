# Folia-rust

[Folia](https://github.com/chthollyphile/folia-major) 歌词可视化音乐播放器的纯原生 Rust 重写，目标是在保留原版界面与动画效果的前提下降低内存、CPU 与 GPU 占用。

## 构建

需要 Rust 1.99.1（由 `rust-toolchain.toml` 固定）；Windows 需要 MSVC 工具链。

```bash
cargo run -p folia-app
```

```bash
cargo build --release -p folia-app
```

发布产物为 `target/release/folia`（Windows 为 `folia.exe`）。

## 文档

- 架构与约定：[docs/architecture.md](docs/architecture.md)

## 许可证

AGPL-3.0，见 [LICENSE](LICENSE)。本项目移植自 chthollyphile 的 [folia-major](https://github.com/chthollyphile/folia-major)（AGPL-3.0），原项目的版权归其作者所有。
