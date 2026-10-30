# 架构

## Crate

依赖方向自上而下，下层不依赖上层。

| crate | 职责 | 依赖 |
|---|---|---|
| `folia-app` | 二进制 `folia`：初始化日志、打开设置、解析语言、启动 iced | ui、store、i18n |
| `folia-ui` | 顶层状态 `Shell` 与 `Message`，`update`/`view`，各页面与控件 | core、store、i18n、iced |
| `folia-store` | 设置持久化：`Store`、`Domain`、`StoreError`、各领域设置 | core |
| `folia-i18n` | 语言包加载与查找：`I18n`、`load` | core |
| `folia-core` | 跨 crate 共用的数据类型：`Language`、`LanguagePreference` | 无 |

尚未创建的 crate 见工作区 `AGENTS.md` 的“架构”与“迁移顺序”。

## 启动流程

1. `folia-app/src/main.rs` 按 `FOLIA_LOG` 或默认级别初始化 `tracing`。
2. `Store::open(APP_DIR)` 定位配置目录：debug 构建为 `Folia-dev`，release 为 `Folia`。
3. 读取 `GeneralSettings`；文件缺失返回默认值，损坏或版本不符时记录警告并使用默认值。
4. `LanguagePreference::resolve` 结合 `sys_locale::get_locale()` 得到实际语言，构造 `I18n`。
5. `iced::application` 以 `Shell::new` 启动，窗口 1200×800，标题取 `app.name`。

## 设置

- 每个领域一个文件，类型实现 `Domain`：

```rust
impl Domain for GeneralSettings {
    const FILE: &'static str = "general.toml";
    const SCHEMA_VERSION: u32 = 1;
}
```

- 文件格式：首行 `schema_version = N`，其后为该领域的字段；结构体加 `#[serde(default)]`，新增字段不需要升版本，改名或改类型必须升版本并写迁移。
- `Store::save` 先写 `<file>.tmp` 再重命名，避免写到一半损坏。
- UI 中改设置后用 `Task::future` 在后台保存，不阻塞界面线程（见 `folia-ui/src/lib.rs` 的 `set_language`）。
- 新增领域：在 `folia-store/src/` 加一个文件定义结构体并实现 `Domain`，在 `lib.rs` 导出。

## 界面文字

- 语言包：`crates/folia-i18n/locales/{en,zh-CN,in,vi}.toml`，TOML 表嵌套，查找时展平成点分键。
- `zh-TW.toml` 由 `build.rs` 用 `zhconv` 从 `zh-CN.toml` 生成到 `OUT_DIR`，不在仓库中。
- `I18n::t(key)` 依次查当前语言、zh-CN，都没有时返回键名本身。
- 新增文字：四份语言包同步加同一个键，键名沿用原版 `folia-major/src/i18n/locales/*.ts` 的路径。

## 开发与发布

| | debug（开发） | release（发布） |
|---|---|---|
| 命令 | `cargo run -p folia-app` | `cargo build --release -p folia-app` |
| 控制台窗口 | 有 | 无（`windows_subsystem = "windows"`） |
| 默认日志 | `debug`（wgpu、naga 为 `warn`） | `info` |
| 设置目录 | `Folia-dev` | `Folia` |
| 优化 | 无 | `lto`、`codegen-units = 1`、`strip` |

开发专用代码放在 cargo feature `dev` 之后，发布构建不启用。

## 测试

测试不在仓库中：放在本地 `test/`（已被 `.gitignore` 忽略），是独立的 cargo workspace，每个被测对象一个子目录。

```bash
cargo test --manifest-path test/Cargo.toml
```

## 当前状态

- 可打开窗口，显示界面语言下拉框（跟随系统 + 5 种语言），切换后立即生效并写入 `general.toml`。
- release 构建空闲：CPU 0%，工作集约 109 MB，私有内存约 174 MB（Windows，Intel Arc A580）。
