# 架构

## Crate

依赖方向自上而下，下层不依赖上层。

| crate | 职责 | 依赖 |
|---|---|---|
| `folia-app` | 二进制 `folia`：日志、目录、读设置、解析语言、命令行文件，启动 iced | ui、store、i18n |
| `folia-ui` | `Shell`（页面、导航、订阅）、音乐库页、搜索页、正在播放页、设置页、播放栏、播放控制 | visualizer、player、omni、library、store、i18n、core |
| `folia-visualizer` | 歌词动画样式（`Style` 注册表、`Scene` canvas 程序、动画曲线） | lyrics、core、iced |
| `folia-omni` | 在线音乐门面 `Omni` 与各平台 `Provider`（目前网易云），在线音频下载缓存 | lyrics、core |
| `folia-player` | 播放引擎：解码线程、重采样、无锁环形缓冲、cpal 输出 | 无 folia 依赖 |
| `folia-library` | 本地音乐库：文件夹扫描、标签、SQLite 索引、旁挂/内嵌歌词 | lyrics、core |
| `folia-lyrics` | 歌词格式识别与解析、翻译匹配、间奏行、渲染提示 | core |
| `folia-store` | 设置持久化：`AppDirs`、`Store`、`Domain`、各领域设置 | core |
| `folia-i18n` | 语言包加载与查找 | core |
| `folia-core` | 共用类型：语言、歌词、曲目、主题、播放队列 | 无 |

尚未创建的 crate（`folia-render`、`folia-analysis`、`folia-mnn-sys`）见工作区 `AGENTS.md`。`folia-render` 在第一个需要自定义 wgpu 着色器的样式移植时创建；流光样式只用 iced canvas。

## 启动流程

1. `folia-app/src/main.rs` 按 `FOLIA_LOG` 或默认级别初始化 `tracing`。
2. `AppDirs::new(APP_DIR)` 取配置/数据/缓存目录：debug 为 `Folia-dev`，release 为 `Folia`。
3. `Store::load_or_default` 读 `general.toml`、`appearance.toml`、`playback.toml`；读不了的记警告并用默认值。
4. `LanguagePreference::resolve` 结合 `sys_locale::get_locale()` 得到语言，构造 `I18n`。
5. 组装 `folia_ui::Init`（含 `library_db = <data>/library.db` 与命令行里的音频文件），`iced::application` 以 `Shell::new` 启动，窗口 1200×800。
6. `Shell::new` 后台读取音乐库索引；有命令行文件时读标签、排队播放并切到正在播放页。

## 线程

| 线程 | 做什么 | 约束 |
|---|---|---|
| UI（iced 事件循环） | `update`/`view`/canvas 绘制 | 不做磁盘、网络、解码 |
| 后台任务 | `background::blocking` 每次起一个线程：扫库、读歌词、存设置 | 结果以消息回到 UI |
| `folia-decoder` | 打开文件、解码、声道映射、重采样、写环形缓冲 | 缓冲满时 10 ms 等待命令 |
| cpal 回调 | 读环形缓冲、乘音量、写设备 | 无分配、无锁、无日志，只碰原子量 |

## 播放

- `Player` 对外只有命令与 `status()`；状态、音量、位置、flush 纪元都在 `shared.rs` 的原子量里。
- 换曲/跳转：解码线程清空自己的缓冲，`request_flush(新位置)`，等回调 `take_flush()` 丢弃旧样本并把位置基准换成新位置（最多 300 ms）；`stop()` 不等待，下次流启动时回调再丢弃。
- 有曲目（含暂停）时 cpal 流保持运行；`stop()` 才暂停流。`load()` 立刻把状态设为 Playing 并清掉上一首的结束标记。
- `folia-ui::playback::Playback` 管队列（`folia_core::Queue<Track>`），收到 Tick 时读 `status()`，Ended 按重复模式前进，Failed 跳到下一首。

## 歌词

- 解析入口：`folia_lyrics::parse(format, content, translation, romanization)`、`parse_file(ext, content, translation)`；格式见 `Format`。
- 每个解析器最后都调 `align::finish`：按开始时间排序、配翻译/音译（±1 s 全局贪心）、插间奏行（间隙 >3 s）。
- `RenderHints::of(line)`：按行时长分 normal / fast / none，给出 `render_end`、逐词提前量、逐词结束；`active_line(lines, t)` 返回当前应显示的行。所有样式都用它，不各写一套。
- 音乐库里的查找顺序：旁挂文件（`lrc vtt ttml qrc yrc krc`，`<stem>.t.<ext>` 为翻译）优先，再内嵌标签；文本按 BOM → UTF-8 → GB18030 解码。

## 在线音乐

- 只经 `folia_omni::Omni`：`search(provider, query, limit, offset)`、`lyrics(&OnlineId)`、`fetch_audio(&OnlineId)`；界面与播放不直接碰平台接口。
- 平台实现 `Provider`（`id`、`display_name`、`search_songs`、`audio_source`、`lyrics`，返回 `BoxFuture`），放 `src/providers/<id>/`，在 `providers/mod.rs` 的 `all()` 里按同名 cargo feature 注册一行。
- 搜索结果直接是 `Track`：`online = Some(OnlineId { provider, id })`，`id` 为 `OnlineId::track_id()`（负数，不与库内行 id 冲突），`path` 在下载前为空。
- 播放在线曲目：`Playback::start_current` 停掉当前曲目，并行取歌词与下载音频（`<cache>/audio/<provider>-<id>.<ext>`，已下载则复用），`AudioReady` 后再 `Player::load`。整段下载后再播放，尚无边下边播。
- 网易云用匿名明文接口（搜索 `/api/cloudsearch/pc`、地址 `/api/song/enhance/player/url`、歌词 `/api/song/lyric/v1`）；weapi `cloudsearch/get/web` 匿名会返回 50000005，见 `Error.md`。登录、歌单、推荐等能力尚未实现。

## 歌词动画样式

- `folia_visualizer::Scene { style, lyrics, time, theme, font_scale }` 实现 `canvas::Program`，背景色 + 按 `style` 分派到 `styles::<style>::draw`。
- 每一帧是播放时间的纯函数：词的状态切换时刻由歌词时间算出，弹簧用解析解、tween 用 CSS cubic-bezier（`anim.rs`），没有逐帧状态；暂停即冻结、跳转即正确。
- 需要测文字的布局按行缓存在 `SceneState` 的 `LayoutCache` 里（按行开始时间、画布尺寸、字号比例取键，最多 4 行）。
- 新增样式：建 `src/styles/<id>/`，在 `registry.rs` 的 `Style` 加一项（id 同原版、显示名 i18n 键），在 `lib.rs` 的 `draw` 分派里加一行，在 `SceneState` 加该样式的缓存字段（如需要）。第二个样式落地时，若分派开始重复，再抽成 trait。
- 流光（classic）与原版的差异：中文按单字排版（原版 CJK 语义分词未移植）；没有 CSS blur（入场/离场只做透明度与缩放）；发光为整词放大光晕而非逐字母扫光；副标题区只显示翻译。

## 设置

| 文件 | 类型 | 字段 |
|---|---|---|
| `general.toml` | `GeneralSettings` | `language`（`system` 或语言代码） |
| `appearance.toml` | `AppearanceSettings` | `theme`（dark/light）、`frame_rate`（30/45/60/90/120/display）、`lyrics_font_scale` |
| `playback.toml` | `PlaybackSettings` | `volume`（0–1）、`repeat`（off/all/one） |

- 每个领域一个文件，类型实现 `Domain`（`FILE`、`SCHEMA_VERSION`）；首行 `schema_version = N`；结构体加 `#[serde(default)]`，新增字段不升版本，改名或改类型必须升版本并写迁移。
- `Store::save` 先写 `<file>.tmp` 再重命名；UI 里用 `background::save(store, &value)` 在后台保存。

## 界面

- 页面：音乐库（`library.rs`）、搜索（`search.rs`），两者共用 `track_list.rs`（只构建视口内的行）；正在播放（`now_playing.rs`）、设置（`settings.rs`）；底部播放栏（`player_bar.rs`）。
- 主题：`folia_core::Theme::{MIDNIGHT, DAYLIGHT}` 为原版两套预设，`Shell` 由它生成 iced 调色板。
- 图标：`lucide-static-svg` 的 `Icon`，经 `icons::icon` / `icon_button` 着色绘制。
- 重绘：只在播放中订阅定时器。正在播放页按帧率上限（`FrameRate::Display` 用 `window::frames()`），其他页 250 ms；不播放时没有任何定时器。
- 界面文字：`crates/folia-i18n/locales/{en,zh-CN,in,vi}.toml`，键名与原版 `folia-major/src/i18n/locales/*.ts` 相同；zh-TW 构建时生成；缺键回退 zh-CN。只有 `options.frameRateDisplay` 是新增键（原版无对应文案）。

## 开发与发布

| | debug（开发） | release（发布） |
|---|---|---|
| 命令 | `cargo run -p folia-app -- [音频文件…]` | `cargo build --release -p folia-app` |
| 控制台窗口 | 有 | 无 |
| 默认日志 | `debug`（wgpu、naga 为 `warn`） | `info` |
| 设置与数据目录 | `Folia-dev` | `Folia` |
| 优化 | 无 | `lto`、`codegen-units = 1`、`strip` |

## 测试

测试不在仓库中：放在本地 `test/`（被 `.gitignore` 忽略），独立 cargo workspace，每个对象一个子目录（`core store i18n lyrics library player visualizer omni`）。`test/omni` 的联网用例标了 `#[ignore]`，用 `cargo test --manifest-path test/Cargo.toml -- --include-ignored` 一起跑。

```bash
cargo test --manifest-path test/Cargo.toml
```

`test/player` 会打开默认音频设备播放静音 WAV（音量 0）；没有设备时自动跳过。

## 当前状态与性能

- 能扫描本地文件夹、播放、暂停、跳转、调音量、切换重复模式；搜索并播放网易云歌曲（含歌词）；显示流光动画与翻译；切换语言、主题、帧率上限。
- release 构建（Windows，Intel Arc A580）：空闲 CPU 0%；正在播放页 60 FPS 动画时 CPU 0.16%（20 逻辑核折算）、GPU 3D 约 3.5%、工作集约 116 MB。
