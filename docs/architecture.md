# 架构

## Crate

依赖方向自上而下，下层不依赖上层。

| crate | 职责 | 依赖 |
|---|---|---|
| `folia-app` | 二进制 `folia`：日志、目录、读设置、解析语言、命令行文件，启动 iced | ui、store、i18n |
| `folia-ui` | `Shell`（页面、导航、订阅）、首页封面墙、搜索页、正在播放页、设置窗口、播放栏、无边框窗口控件 | render、visualizer、player、omni、library、lyrics、store、i18n、core |
| `folia-render` | 全屏 WGSL 通道（画在当前渲染通道上）：正在播放页的暗角、整数帧颗粒、字幕区主题色径向渐变；首页墙面的底色光晕（`Wash`）与暗角颗粒（`Veil`） | iced、wgpu |
| `folia-visualizer` | 歌词动画样式（`Style` 注册表、`Scene` canvas 程序、动画曲线） | lyrics、core、iced |
| `folia-omni` | 在线音乐门面 `Omni` 与各平台 `Provider`（目前网易云），在线音频下载缓存 | lyrics、core |
| `folia-player` | 播放引擎：解码线程、重采样、无锁环形缓冲、cpal 输出 | 无 folia 依赖 |
| `folia-library` | 本地音乐库：文件夹扫描、标签、SQLite 索引、旁挂/内嵌歌词、封面（`cover_for`） | lyrics、core |
| `folia-lyrics` | 歌词格式识别与解析、翻译匹配、间奏行、渲染提示 | core |
| `folia-store` | 设置持久化：`AppDirs`、`Store`、`Domain`、各领域设置 | core |
| `folia-i18n` | 语言包加载与查找 | core |
| `folia-core` | 共用类型：语言、歌词、曲目、主题、播放队列 | 无 |

尚未创建的 crate（`folia-analysis`、`folia-mnn-sys`）见工作区 `AGENTS.md`。歌词样式仍用 iced canvas。`folia-render` 负责正在播放页的暗角、颗粒，以及字幕背后的主题色径向渐变，还有首页墙面的底色光晕与暗角颗粒。所有通道共用 `pipeline::Fullscreen`（一个全屏三角形 + 一个 uniform），共用的顶点着色器与像素哈希在 `shaders/fullscreen.wgsl`，由 `concat!` 拼到各着色器前。不采样交换链，也不修改 `iced_wgpu`。

## 启动流程

1. `folia-app/src/main.rs` 按 `FOLIA_LOG` 或默认级别初始化 `tracing`。
2. `AppDirs::new(APP_DIR)` 取配置/数据/缓存目录：debug 为 `Folia-rust-dev`，release 为 `Folia-rust`（原 Electron 版占用 `%APPDATA%\Folia`，不得复用）。
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
- 已有样式：`classic`（流光）、`partita`（云阶）、`still`（静止，帧率上限 10）、`tilt`（倾诉）、`claddagh`（回环，字素排在倾斜椭圆上）、`cadenza`（心象，当前行的词散在一个更大的焦点词周围）。
- 尚未移植的样式按原版非空行数从小到大。行数是 `src/components/visualizer/<id>/` 里 `.ts`/`.tsx`/`.css` 的非空行。着色器指 GLSL、WebGL、Pixi、three.js 或 Paper shader；Canvas 2D 不算。

| 行数 | id | 显示名 | i18n 键 | 着色器 |
|---|---|---|---|---|
| 1152 | `claddagh` | 回环 | `ui.visualizerCladdagh` | 否。DOM 变换和 CSS drop-shadow |
| 2013 | `cappella` | 群唱 | `ui.visualizerCappella` | 否。自带头像和表情图 |
| 3776 | `monet` | 莫奈 | `ui.visualizerMonet` | 否。Canvas 2D |
| 3967 | `nebbia` | 雾笺 | `ui.visualizerNebbia` | 是。自带 GLSL。`entry` 里 `background: false` |
| 4086 | `fume` | 浮名 | `ui.visualizerFume` | 否。Canvas 2D。`entry` 会改 common 背景 |
| 5052 | `pendolo` | 时计 | `ui.visualizerPendolo` | 是。机芯用 WebGL |
| 6570 | `diorama` | 镜台 | `ui.visualizerDiorama` | 是。three.js 和粒子着色器 |
| 9869 | `lumiere` | 绘光 | `ui.visualizerLumiere` | 是。Pixi |
| 10808 | `sonnet` | 商籁 | `ui.visualizerSonnet` | 是。Pixi |
| 11725 | `tempera` | 凝彩 | `ui.visualizerTempera` | 是。Pixi |
| 12336 | `hanabi` | 花火 | `ui.visualizerHanabi` | 是。Pixi |
| 12815 | `machi` | 町景 | `ui.visualizerMachi` | 是。自带场景着色器。`entry` 里 `background: false` |

- 背景（`visualizer/backgrounds/`，同样按非空行）：`sora` 320 行，着色器星空；`url` 355 行，嵌入页面，不自带着色器；`latent` 729 行，Paper 着色器；`monet` 883 行，Canvas 2D 封面；`nomand` 983 行，Paper 着色器；`common` 1106 行，CSS 几何和封面柔焦，不自带着色器。只有浮名在 entry 里改背景，所以 `common` 放在浮名之前。星空、隐现、漫游没有样式写死要它们。雾笺、町景、静止关掉背景。
- 心象（cadenza）与原版的差异：字素用 Rust `char`，不是 `Intl.Segmenter`。折行用 `measure` 贪心，优先在空白断开，不是 pretext，也没有 pretext 的字距。缩放、转角、透明度按原版指数速率 11 和 14，从这一行出现和词状态切换的时刻解析计算；发光包络、正文混色和唱完漂移直接取原版曲线。节拍能量固定为 0，所以每个词的正弦浮动为 0，整云仍有 `sin(time × 2.3) × 3` 像素的上下浮动。没有 CSS `blur()` 和 40px `text-shadow`，发光是放大 1.08 的半透明强调色；普通揭示下的非中日韩多字词按字素分别发光。没有逐词配色，强调色用主题 `accent`。没有 `isChorus`，不画副歌波纹。`beamIntensity` 固定 0。调节项用默认值（`fontScale` 1.12、`widthRatio` 0.72、`motionAmount` 1、`glowIntensity` 1），设置面板未做。不画「等待音乐」。底部字幕仍走共用叠层。只画渲染窗口里的当前行。行的进入、退出和停顿时长来自 `RenderHints::phases`。
- 回环（claddagh）与原版的差异：字素用 Rust `char`，不是 `Intl.Segmenter`。字宽用 `measure`（Bold 72px），再加原版 0.04em 字距和 0.18em 字距。换行弹簧是 `anim::spring`，刚度 55/0.9、阻尼 14/0.9。每一行从上一段在换行时刻已经转到的角度接着转，短于收敛时间的行不会跳回上一行的目标角；中心行序号一次跳过超过 1 行时直接到目标角。原版拖动进度拿上一帧的序号比较，这里一帧只由播放时间决定，按顺序开始的歌词在拖动后显示连续播到该时刻的角度。第一行之前原版中心序号是 -1，进入第 0 行会从 -π 转过来；第 0 行保持 0。有行开始之后，中心行就是最后一个已开始的行。节拍能量固定为 0（还没有频段），中轴线不随低音伸缩，副歌错位不出现。没有 `isChorus`。远处字的 `blur()` 和多层 `text-shadow` 没画，只保留透明度、缩放、切线转角和唱到后的强调色。调节项用默认值（`focusScaleRatio` 0.65、`radiusScale` 1、`ellipseTiltDeg` 45、显示中轴线、`letterSpacingOffset` 0），设置面板未做。
- 新增样式：建 `src/styles/<id>/`（或单文件 `<id>.rs`），在 `registry.rs` 的 `Style` 加一项（id 同原版、显示名 i18n 键、`max_fps`），在 `lib.rs` 的 `draw` 分派里加一行，在 `SceneState` 加该样式的缓存字段（如需要）。目前两个样式只在 `draw` 一处分派，尚不需要 trait。
- `random`：每首歌用 `Style::random_for(track.id)` 稳定地抽一个样式（随机池为全部样式，暂不可编辑）。
- 正在播放页的重绘频率 = min(设置的帧率上限, 样式的 `max_fps`)；两者都不限时用 `window::frames()`。
- `styles/glow.rs` 是原版 GlowWord 的移植，流光与云阶共用：逐词姿态 `word_pose`、行进出 `visible_lines`、呼吸 `breathing`、绘制 `draw_word`、翻译 `draw_translation`；新样式用到逐词发光时直接调用，不复制。
- 显示分组：`folia_lyrics::layout_units(line, semantic)` + `display_words` 同原版 `buildPostLyricLayoutUnits` / `buildDisplayWordsFromLayoutUnits`：CJK 用 icu 词典分词对齐计时词（语义词保留逐字计时），标点与英文缩写粘到前一单元；样式要排版逐词时都用它。
- 流光（classic）与原版的差异：没有 CSS blur（入场/离场只做透明度与缩放）；发光为整词放大光晕而非逐字母扫光。底部字幕区的译文和下一句由正在播放页的叠层负责，见「窗口」。
- 倾诉（tilt）与原版的差异：字宽用 iced 测量（原版 pretext）；调节面板未移植，概率与配色用默认值（拆行 0.75、斜体行 0.35、配色 default：普通行 primary、斜体行 accent）；中日韩等没有斜体字形的字用正体 Light，再按 Chrome/Blink 的 14° 斜体斜率做剪切（拉丁字母仍用字体自带斜体）。剪切后的轮廓每帧由 canvas 重新三角化，网格缓存留到 `folia-render`。逐字缩放直接取脉冲值，没有原版 0.06s 的 CSS 过渡；整列进入透明度 0.3s 线性，离开 0.45s ease-in-out，同时各行按各自入场时长收到 y -12（普通）或 y -16 且 scale 0.95（斜体）；退场时可见行数冻结在退场时刻；一行最多 `MAX_ROWS`（4）行；句子不够分时的二次切分用 ICU 词典分词；歌词块中心在画布高度的 50%。句子切分是 `folia_lyrics::split_sentences`，JS 空白判断是 `folia_lyrics::is_js_space`。退场时刻与流光共用 `glow::exit_at`。

## 窗口

- 无边框：`decorations: false`，最小 350×100。Windows 上 `undecorated_shadow: true`（保留阴影和贴边分屏用的厚边框）、`corner_preference: Round`。亚克力背景默认关，与原版 `enable_player_page_native_blur` 默认 false 一致，尚未做开关。
- 四边 6px、四角 14px 为缩放热区，调用 `drag_resize`，光标分别是垂直、水平、两条对角线。顶上 32px 空白在指针移动超过 4px 后才 `drag`，避免按下立刻进入系统移动循环、把双击吃掉；双击这条空白切换最大化。右上角四个按钮各 44×32，从左到右：全屏（`Maximize` / `Minimize`，13px）、最小化、最大化/还原（方框 / Copy）、关闭（悬停变红）。F11 切换全屏（`window::Mode::Fullscreen` / `Windowed`），忽略按键重复。导航右侧留白 192px，给这四个按钮让位。贴边分屏后窗口尺寸变化会再问一次 `is_maximized`，用来同步还原图标。
- 正在播放页歌词铺满窗口。导航、窗口按钮、底部胶囊的显隐由 `ChromeClock` 决定：只有指针坐标和上次记录的不同才把期限延长 3 秒（布局变化会把同一个坐标再送一次，不能据此重新显示）；滚轮、按下指针、进入正在播放页延长 3 秒；指针离开窗口 300ms 后隐藏（同 `usePlayerChromeAutoHide` 的 3000 / 300）。按住指针时不隐藏。期限用 `ChromeTick` 每 200ms 检查，只在 `until_ms` 还有值时订阅。窗口 id 在启动时用 `iced::window::oldest()` 取一次，取不到时下一次用到再取。其他页面这些条保持常显。导航画在拖动条之上，避免标签被拖动热区吃掉。
- 左上角返回是 40×40 圆按钮（`ChevronLeft` 20px，`bg-black/20`，图标白 60%，距顶距左各 24px），只在指针进入左上 120×120 热区时出现，与原版 `VisualizerShell` 的 `PLAYER_CHROME_HOTSPOT_SIZE` 相同。右下角设置是 48×48（`Settings2` 20px，暗色 `bg-black/40`、日光 `bg-white/70`，右边 32px、底边 32px），跟胶囊一起隐藏。它打开设置窗口；原版这颗按钮打开的是播放器面板，面板还没移植。
- 底部胶囊最宽 512px（`32rem`），宽为 `clamp(窗口宽 - 32, 280, 512)`，窗口宽 ≥768 时收起为 60%，否则 80%。底边 32px（`bottom-8`），圆角 28。播放中默认收起，只留进度（时间加滑条，`px-4 py-2`）；悬停 20ms 后展开，移出 100ms 后收起；有曲目且不在播放时保持展开（同 `FloatingPlayerControls` 的 `showExpanded`）。宽度在 260ms 内线性走完再套 cubic ease-out `1-(1-t)^3`。展开后暗色填充黑 40%、白边 5%，日光填充白 60%、白边 20%；收起时填充降到黑 20% 或白 40%。上排是标题和按钮，下排是进度条。
- 底部字幕同时放当前行译文和下一句可读文本（跳过 `......` 这类没有字母数字的行）。译文 `clamp(18px, 2.6vw, 20px)` 乘字号比例，透明度 0.6。下一句排在译文下面时字号为译文的 0.88、透明度为 0.6×0.82、间距 0.25em（`SECONDARY_TRACK_*`）；没有译文时用 `clamp(14px, 2vw, 16px)`、透明度 0.6。左右边距 16px。控制条在时底边 112px（基线 32 + 净空 80），隐藏后 32px。静止样式不重复画。每行底下有一层下移 1px、透明度 0.24×行透明度的主题背景色，近似 `text-shadow: 0 1px 2px`。字幕底在 `atmosphere.wgsl` 里直接画：文字框宽度用 `folia_visualizer::measure`（字号与该行相同，`Font::DEFAULT`，字重 Normal，上限 896px）。椭圆为这个文字框（加 `px-1.5 py-0.5` 和 `-inset-x-10 -inset-y-6`）的 115% × 130%，颜色是 `theme.background`，色标 alpha 在中心 0.96、62% 处 0.78、边缘 0，再把外沿平滑延出 40px 近似 `blur-2xl`。整块再乘字幕区透明度 0.6，叠在暗角之上。暗角强度 0.28，颗粒 0.04。颗粒的哈希是 u32 xorshift，帧号为 `time * 48` 对 4096 取模。没有远程控制按钮和点击穿透。亚克力仍默认关。
- 与原版的差异：暗底的 40px 模糊是把椭圆外沿的色标过渡加宽，不是对渐变做高斯卷积。下一句没有 `blur(1px)` 的字形模糊。文字阴影没有 2px 模糊半径，只有下移 1px 的纯色副本。椭圆宽度用 iced 测量，字体是界面默认的 `Font::DEFAULT`，不是原版译文那套字体栈。原版胶囊内容切换是 layout spring（stiffness 280、damping 24），整条隐藏才是 0.26s ease-out；这里用这 260ms 来改宽度。原版默认字幕布局只有译文，下一句是布局选项；这里固定为译文加下一句。没有和原版并排截图（原版播放器在运行，没有关它，也没有改它的设置）。

## 设置

| 文件 | 类型 | 字段 |
|---|---|---|
| `general.toml` | `GeneralSettings` | `language`（`system` 或语言代码） |
| `appearance.toml` | `AppearanceSettings` | `theme`（dark/light）、`frame_rate`（30/45/60/90/120/display）、`lyrics_font_scale`、`visualizer`（样式 id 或 `random`，默认 `classic`） |
| `playback.toml` | `PlaybackSettings` | `volume`（0–1）、`repeat`（off/all/one） |

- 每个领域一个文件，类型实现 `Domain`（`FILE`、`SCHEMA_VERSION`）；首行 `schema_version = N`；结构体加 `#[serde(default)]`，新增字段不升版本，改名或改类型必须升版本并写迁移。
- `Store::save` 先写 `<file>.tmp` 再重命名；UI 里用 `background::save(store, &value)` 在后台保存。

## 界面

- 页面：首页（`library.rs` + `wall.rs`，原版 Bravais 封面墙）、搜索（`search.rs`，用 `track_list.rs`，只构建视口内的行）；正在播放（`now_playing.rs`）；底部播放栏（`player_bar.rs`）。设置不是页面，是盖在任何页面上的窗口（`settings.rs` + `settings_kit.rs`）。
- 非播放页的顶栏就是标题栏：`chrome::drag_area` 包住导航行，空白处拖动窗口、双击最大化，标签照常点击；正在播放页用同一函数生成的顶部拖动条。导航行右端留 192px 给窗口按钮（`WINDOW_BUTTONS`），“选项”不再被最大化按钮盖住。
- 设置窗口照原版 `SettingsModal` 与 `kit/`：遮罩黑 50%（日间 32%），窗口 960px 宽（不超过窗口宽减 40）、高 86%，圆角 16、1px `line2` 描边、`0 36px 90px` 50% 黑阴影，打开/关闭 0.24s easeOut，透明度 0→1、上移 20px（iced 不能整棵子树变透明，`Tokens::faded` 把所有颜色乘上透明度）。左侧 212px 分组列表（搜索框、分组名 12px、条目 32px 高 13px 字 16px 图标，当前项 `fill2`），右侧标题 20px Semibold、关闭按钮 32px、多组页面有标签条（13px、间距 22、2px 强调色下划线滑动 0.22s），内容区左右 32px、底部 64px。小于 768px 时列表变成顶部横条（无图标），边距减半。颜色取 `settingsTokens`：表面色按明暗固定，文字与填充是主题文字色的 62/42/8/16/7/12/17% 透明度，强调色是主题 `accent`，强调色上的字按 WCAG 对比度在 `#17171a` 与白之间选。控件：行（最小 56px、左右 16px、子行左缩进 40、上方发丝线）、卡片（圆角 12、1px `line`）、开关（40×24，拇指 18px，0.22s 滑动）、分段选择（高 32、内边距 2、选中块 `raise` 带阴影并滑动）、下拉（iced `pick_list`：32px 高、`fill` 底、菜单 `#2b2b30`/白、圆角 10、`0 14px 36px` 阴影）。Esc 先清空搜索再关闭，下拉菜单开着时 Esc 只关菜单。
- 设置页面只放 Folia-rust 已有的功能：外观（歌词动画样式、明暗模式）、通用（界面语言，跟随系统时提示当前生效语言）、性能与动效（帧率限制开关，打开后子行选 30/45/60/90/120，开关打开默认 120，与原版 `FRAME_RATE_WHEN_SWITCHED_ON` 相同；关闭即跟随显示器）。搜索在分类名、组名、选项名里做不区分大小写的子串匹配，结果行右侧显示“分类 › 组”，点击跳到该分类并滚到该组。
- 设置窗口与原版的差异：没有帮助页和其余 9 个分类；歌词动画行原版是打开动画工作台的链接，这里直接是下拉；明暗模式没有“跟随系统”；搜索没有命令面板的同义词与拼音；下拉菜单与触发器同宽、从左边缘垂下，选中项没有对勾；“i”展开说明、条目悬停的 tooltip、焦点环、拇指按下变宽、子行展开动画都没有；标签条跟随滚动按估算的组高度判断。
- 首页封面墙：格子 128px、间距 8px，12×8 格的块，4 个模板 × 4 种镜像（`mixBlockCoords` 用 32 位乘法照搬），槽位按 `|dx| + |dy|·0.9 − √面积·136·0.35` 排序后依次放曲目；整体缩放 <640px 0.52、<1100px 0.64、其余 0.76。层次自下而上：`Wash`（底色 + 左上 accent 16%、右下 secondary 20% 两团光晕）、墙面 canvas、`Veil`（暗角 52%/日间 16%，静态颗粒）、右上工具按钮。磁贴：底色为背景混 12%（日间 6%）文字色，封面按 `object-fit: cover` 在解码时裁成磁贴比例（`folia_library::cover_for`，只缩不放），自上而下 5%→5%(34%)→24%(56%)→92% 的黑（日间白）渐变，左上 14px 编号角标，左下 17px 标题（`clamp(22px, 2vw, 36px)`，行高 0.96，最多 3 行）与歌手（13px，单行），正在播放加 2px accent 描边。
- 墙面性能：canvas 自己发 `Viewport` 消息上报尺寸；标题断行与省略只在磁贴进入视口或字号变化时测一次（`Label` 缓存），封面只解码视口内磁贴，最多保留 120 张（LRU）。绘制全部用窗口坐标：iced 0.14 的 `Frame::with_clip` 不会把图片和文字平移进裁剪区。
- 首页与原版的差异：没有原版的块底板与空画框、正在播放大格子的进度条、悬停/聚焦动效、标题字距 −0.045em（iced 不支持字距）；颗粒是逐像素哈希的黑白点而非 soft-light 分形噪声；导航栏与播放栏仍是现有布局，原版首页是全屏墙面。
- 主题：`folia_core::Theme::{MIDNIGHT, DAYLIGHT}` 为原版两套预设，`Shell` 由它生成 iced 调色板。
- 图标：`lucide-static-svg` 的 `Icon`，经 `icons::icon` / `icon_button` 着色绘制。
- 重绘：播放定时器只在播放中订阅。正在播放页按帧率上限（`FrameRate::Display` 用 `window::frames()`），其他页 250 ms。浮层的 200 ms 检查只在隐藏期限还在时订阅；胶囊的 16 ms 只在悬停延迟或宽度动画还没结束时订阅。不播放且浮层已隐藏时没有定时器。
- 界面文字：`crates/folia-i18n/locales/{en,zh-CN,in,vi}.toml`，键名与原版 `folia-major/src/i18n/locales/*.ts` 相同；zh-TW 构建时生成；缺键回退 zh-CN。没有原版之外的新增键。插值用 `I18n::format`（`{{name}}`，与 i18next 相同）。

## 开发与发布

| | debug（开发） | release（发布） |
|---|---|---|
| 命令 | `cargo run -p folia-app -- [音频文件…]` | `cargo build --release -p folia-app` |
| 控制台窗口 | 有 | 无 |
| 默认日志 | `debug`（wgpu、naga 为 `warn`） | `info` |
| 设置与数据目录 | `Folia-rust-dev` | `Folia-rust` |
| 优化 | 无 | `lto`、`codegen-units = 1`、`strip` |

## 测试

测试不在仓库中：放在本地 `test/`（被 `.gitignore` 忽略），独立 cargo workspace，每个对象一个子目录（`core store i18n lyrics library player visualizer omni ui render`）。`test/omni` 的联网用例标了 `#[ignore]`，用 `cargo test --manifest-path test/Cargo.toml -- --include-ignored` 一起跑。浮层显隐、胶囊宽度和字幕行在 `test/ui`；颗粒帧号在 `test/render`。

```bash
cargo test --manifest-path test/Cargo.toml
```

`test/player` 会打开默认音频设备播放静音 WAV（音量 0）；没有设备时自动跳过。

## 当前状态与性能

- 首页为原版 Bravais 封面墙，可拖动、滚轮平移、点击播放；能扫描本地文件夹、播放、暂停、跳转、调音量、切换重复模式；搜索并播放网易云歌曲（含歌词）；显示流光/云阶/静止/倾诉/回环/心象歌词样式与翻译；切换语言、主题、歌词样式、帧率上限。
- release 构建（Windows，Intel Arc A580）：空闲 CPU 0%；正在播放页 60 FPS 动画时 CPU 0.16%（20 逻辑核折算）、GPU 3D 约 3.5%、工作集约 116 MB。首页封面墙（26 首）静止时 CPU 0%、GPU 0%，持续拖动时 CPU 0.06%、GPU 3D 3.7%，工作集约 130 MB、私有内存约 230 MB。
