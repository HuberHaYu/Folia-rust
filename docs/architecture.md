# 架构

## Crate

依赖方向自上而下，下层不依赖上层。

| crate | 职责 | 依赖 |
|---|---|---|
| `folia-app` | 二进制 `folia`：日志、目录、读设置、解析语言、命令行文件，启动 iced | ui、store、i18n |
| `folia-ui` | `Shell`（页面、导航、订阅）、首页封面墙与在线页签、登录窗口、搜索页、正在播放页、设置窗口、播放栏、无边框窗口控件 | render、visualizer、player、omni、library、lyrics、store、i18n、core |
| `folia-render` | 全屏 WGSL 通道（画在当前渲染通道上）：正在播放页的暗角、整数帧颗粒、字幕区主题色径向渐变；首页墙面的底色光晕（`Wash`）与暗角颗粒（`Veil`） | iced、wgpu |
| `folia-visualizer` | 歌词动画样式（`Style` 注册表、`Scene` canvas 程序、动画曲线） | lyrics、core、iced |
| `folia-omni` | 在线音乐门面 `Omni` 与各平台 `Provider`（目前网易云）：搜索、歌词、音频下载缓存、扫码登录、账户、歌单、收藏专辑、电台、探索；会话只存系统密钥库 | lyrics、core |
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
6. `Shell::new` 后台读取音乐库索引；有命令行文件时读标签、排队播放并切到正在播放页。窗口第一帧起盖首屏遮罩（`splash.rs`）。首页第一次收到资料库加载结果（成功或失败）并且之后画出一帧时开始 320ms 淡出；内容淡入仍从第一帧起算（延迟 180ms、时长 420ms），就绪早于 180ms 时内容不出现。逐帧订阅只到淡出结束。

## 线程

| 线程 | 做什么 | 约束 |
|---|---|---|
| UI（iced 事件循环） | `update`/`view`/canvas 绘制 | 不做磁盘、网络、解码 |
| 后台任务 | `background::blocking` 每次起一个线程：扫库、读歌词、存设置、读写系统密钥库、解码在线封面、扫码轮询之间的 2 秒等待 | 结果以消息回到 UI |
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

- 只经 `folia_omni::Omni`：`search`、`lyrics`、`fetch_audio`、`fetch_cover(url, px)`，以及 `platforms`、`login_qr`、`check_login`、`account`、`logout`、`restore_session` / `save_session` / `forget_session`、`user_playlists`、`collection_detail`、`set_subscribed`、`favorite_albums`、`radio_feed`、`explore`。界面与播放不直接碰平台接口。
- 平台实现 `Provider`（`lib.rs` 的 trait，异步方法返回 `BoxFuture`）：`id`、`display_name`、`search_songs`、`audio_source`、`lyrics`、`login_code`、`check_login`、`account`、`logout`、`session`、`restore_session`、`user_playlists`、`collection_detail`、`set_subscribed`、`favorite_albums`、`radio_feed`、`explore`、`sized_cover`。放 `src/providers/<id>/`，在 `providers/mod.rs` 的 `all()` 里按同名 cargo feature 注册一行。
- 门面类型：`Platform`（原版 `providerRegistry.ts` 注册的 7 个平台：网易云、酷狗、QQ音乐、波点、YT Music、Spotify、汽水，带各自声明的记号字与颜色；`available` 表示编进了 provider，目前只有网易云）、`LoginQr`（轮询用的 key 与二维码模块矩阵；界面拿不到二维码里的地址）、`LoginState`（Waiting / Scanned / Confirmed / Expired / Failed{code, message}）、`Account`、`Collection`（Playlist / Album / DailySongs / PersonalFm；`wall_id()` 是墙用的负数 id）、`RadioFeed`（私人 FM、每日推荐、推荐歌单；`partial` 表示有一块失败）、`ExploreSection`（`Shelf`：每日推荐 / 每日推荐歌单 / 排行榜 / 平台块标题）。错误新增 `SignedOut`（平台码 301）、`Qr`、`Keyring`。
- `collection_detail` 返回 `CollectionDetail { tracks, subscribed, owner }`：网易云歌单读 `/playlist/subscribed` 与 `/playlist/creator/userId`，专辑与 FM、每日推荐不带收藏状态。`set_subscribed` 收藏 / 取消收藏歌单或专辑（星标）。私人 FM 的集合每读一次 `collection_detail` 就是新的一批（3 首）。
- 封面：`fetch_cover(url, px)` 先经 provider 的 `sized_cover` 换成按尺寸取的地址（网易云 `126.net`：`?param=NyN`，即 `getSizedCoverUrl`；墙按 `COVER_SIZE_STEPS` 256/512/1024 取档），未命中缓存时先拿 `tokio::sync::Semaphore` 的许可（`COVER_DOWNLOADS = 6`），拿到后再查一次缓存。墙按离视口中心的距离排下载顺序，平移后离开视口的下载被中止（`Task::abortable`）。
- 探索「换一批」：网易云声明 `refreshReadsAnew`，所以每次 `explore` 都是从头重读。各栏照 `handOut` 交一批（12 个）：`reads.rs` 的 `HandedOut`（`exploreReads.ts` 的 `createHandedOut`）按栏记交过的条目，没交过的排前，其余按最久以前交过的在前；每个范围最多记 5000 个。账户有无变化时整份记忆重来。界面把新一页按栏 id 合并：还在的栏换成新条目，网易云不再给的栏留着原条目，新栏接在后面；回答到之前墙上留着现在的条目。
- 搜索结果直接是 `Track`：`online = Some(OnlineId { provider, id })`，`id` 为 `OnlineId::track_id()`（负数，不与库内行 id 冲突），`path` 在下载前为空。
- 私人 FM（`Playback::play_fm`）：队列标成 FM，重复模式在 FM 里按关（`queueLoopMode`）；往下一首走、且当前是倒数第二首或最后一首时（`fmNeedsTopUpAt`），先用 FM 集合再读一批，只追加队列里还没有的歌（`freshFmSongs`），再前进；读失败照样往下走，到尾就停。等这一批时又换了歌或换了队列，这一批作废。电台页签的 FM 卡第二行是当前模式名（`personalFmModeLabel`；没有模式选择器，所以是 `personalFmMode.mode.DEFAULT`）。
- 播放在线曲目：`Playback::start_current` 停掉当前曲目，并行取歌词与下载音频（`<cache>/audio/<provider>-<id>.<ext>`，已下载则复用），`AudioReady` 后再 `Player::load`。整段下载后再播放，尚无边下边播。
- 网易云的搜索、播放地址、歌词也走 eapi，带上已登录的会话（没登录时就是游客会话）：搜索 `/api/cloudsearch/pc`（`total: true`）；播放地址 `/api/song/enhance/player/url/v1`（`song_url_v1`，`level` 用原版 `getSongUrl` 的默认值 `exhigh`，`encodeType: flac`）；歌词 `/api/song/lyric/v1`（`lyric_new` 的参数）。之前用不带 cookie 的明文请求时，登录后要 VIP 或登录才能放的歌还是拿不到地址。原版的 `randomCNIP`（伪造国内 IP 头）没有照搬。weapi `cloudsearch/get/web` 匿名会返回 50000005，见 `Error.md`。
- 网易云的登录与账户数据走客户端接口 eapi（`providers/netease/eapi.rs`）：`POST https://interface.music.163.com/eapi/<路径>`，表单 `params` 是「路径`-36cd479b6b5-`JSON`-36cd479b6b5-`MD5(`nobody`路径`use`JSON`md5forencrypt`)」的 AES-128-ECB（PKCS#7，密钥 `e82ckenh8dichen8`）大写十六进制；JSON 里的 `header` 和 Cookie 带 `os=pc`、`appver`、`osver`、`deviceId`、`__csrf`、`MUSIC_U` 等字段，与原版后端 NeteaseCloudMusicApi 的 eapi 相同。算法与 Node `crypto` 写的参照实现逐字节比对过。加密与原始接口只在 `providers/netease/`。
- 用到的网易云接口（对应原版 `netease.ts`）：二维码 key `/api/login/qrcode/unikey`（`type: 3`）；二维码内容 `https://music.163.com/login?codekey=<key>&chainId=v1_<deviceId>_web_login_<毫秒>`；轮询 `/api/login/qrcode/client/login`（800 过期、801 等待、802 已扫码、803 确认，确认时从 `Set-Cookie` 取 `MUSIC_U` 与 `__csrf`）；账户 `/api/nuser/account/get`；退出 `/api/logout`；用户歌单 `/api/user/playlist`（每页 50，翻完）；歌单曲目 `/api/v6/playlist/detail` 取 id，再 `/api/v3/song/detail` 每 500 个一批；专辑曲目 `/api/v1/album/<id>`；收藏专辑 `/api/album/sublist`（每页 50，翻完，`FAVORITE_ALBUMS_PAGE_SIZE`）；私人 FM `/api/v1/radio/get`；每日推荐 `/api/v3/discovery/recommend/songs`；推荐歌单 `/api/personalized/playlist`（35 个，`HOME_FEED_RECOMMENDATION_LIMIT`）；探索首页 `/api/homepage/block/page`、每日推荐歌单 `/api/v1/discovery/recommend/resource`、排行榜 `/api/toplist`。电台三块和探索各部分各自成败，全部失败才算失败（`getHomeFeed`）。
- 会话（`deviceId`、`MUSIC_U`、`__csrf` 的 JSON）只存系统密钥库：`keyring` 4（Windows 凭据管理器、macOS 钥匙串、Linux Secret Service），服务名是应用目录名（debug `Folia-rust-dev`、release `Folia-rust`），条目 `<provider>-session`（`session.rs`）。不写设置文件，不进日志和界面。密钥库调用会阻塞，所以 `Omni` 的这三个方法是同步的，界面只在 `background::blocking` 的线程里调用。

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
| `general.toml` | `GeneralSettings` | `language`（`system` 或语言代码）、`home_tab`（playlist/radio/explore/albums/local，默认 playlist，同原版 `LAST_HOME_VIEW_TAB_KEY`） |
| `appearance.toml` | `AppearanceSettings` | `theme`（dark/light）、`frame_rate`（30/45/60/90/120/display）、`lyrics_font_scale`、`visualizer`（样式 id 或 `random`，默认 `classic`） |
| `playback.toml` | `PlaybackSettings` | `volume`（0–1）、`repeat`（off/all/one） |

- 每个领域一个文件，类型实现 `Domain`（`FILE`、`SCHEMA_VERSION`）；首行 `schema_version = N`；结构体加 `#[serde(default)]`，新增字段不升版本，改名或改类型必须升版本并写迁移。
- `Store::save` 先写 `<file>.tmp` 再重命名；UI 里用 `background::save(store, &value)` 在后台保存。

## 界面

- 页面：首页（`library.rs` + `wall.rs` + `home/`，原版 Bravais 封面墙和浮动外壳）、搜索（`search.rs`，全屏浮层）、正在播放（`now_playing.rs`，胶囊与左上角返回）。设置是盖在任何页面上的窗口（`settings.rs` + `settings_kit.rs`）。早期顶栏（全部歌曲 / 搜索 / 歌词动画 / 选项）和底部播放栏已去掉；`player_bar::view` 已删，胶囊还在。
- 窗口拖动：首页在墙面之上放一条 32px 拖动条，缝和窗口按钮叠在它上面。搜索浮层用 `chrome::drag_area` 包住，空白处可拖。正在播放页用 `chrome::drag_strip`。窗口按钮仍是右上角 44×32。
- 设置窗口照原版 `SettingsModal` 与 `kit/`：遮罩黑 50%（日间 32%），窗口 960px 宽（不超过窗口宽减 40）、高 86%，圆角 16、1px `line2` 描边、`0 36px 90px` 50% 黑阴影，打开/关闭 0.24s easeOut，透明度 0→1、上移 20px（iced 不能整棵子树变透明，`Tokens::faded` 把所有颜色乘上透明度）。左侧 212px 分组列表（搜索框、分组名 12px、条目 32px 高 13px 字 16px 图标，当前项 `fill2`），右侧标题 20px Semibold、关闭按钮 32px、多组页面有标签条（13px、间距 22、2px 强调色下划线滑动 0.22s），内容区左右 32px、底部 64px。小于 768px 时列表变成顶部横条（无图标），边距减半。颜色取 `settingsTokens`：表面色按明暗固定，文字与填充是主题文字色的 62/42/8/16/7/12/17% 透明度，强调色是主题 `accent`，强调色上的字按 WCAG 对比度在 `#17171a` 与白之间选。控件：行（最小 56px、左右 16px、子行左缩进 40、上方发丝线）、卡片（圆角 12、1px `line`）、开关（40×24，拇指 18px，0.22s 滑动）、分段选择（高 32、内边距 2、选中块 `raise` 带阴影并滑动）、下拉（iced `pick_list`：32px 高、`fill` 底、菜单 `#2b2b30`/白、圆角 10、`0 14px 36px` 阴影）。Esc 先清空搜索再关闭，下拉菜单开着时 Esc 只关菜单。
- 设置页面只放 Folia-rust 已有的功能：外观（歌词动画样式、明暗模式）、通用（界面语言，跟随系统时提示当前生效语言）、性能与动效（帧率限制开关，打开后子行选 30/45/60/90/120，开关打开默认 120，与原版 `FRAME_RATE_WHEN_SWITCHED_ON` 相同；关闭即跟随显示器）。搜索在分类名、组名、选项名里做不区分大小写的子串匹配，结果行右侧显示“分类 › 组”，点击跳到该分类并滚到该组。
- 设置窗口与原版的差异：没有帮助页和其余 9 个分类；歌词动画行原版是打开动画工作台的链接，这里直接是下拉；明暗模式没有“跟随系统”；搜索没有命令面板的同义词与拼音；下拉菜单与触发器同宽、从左边缘垂下，选中项没有对勾；“i”展开说明、条目悬停的 tooltip、焦点环、拇指按下变宽、子行展开动画都没有；标签条跟随滚动按估算的组高度判断。
- 墙的换层（`wall_shift.rs`，取数自 `bravaisShift.ts`、`flipPlan.ts`、`bravaisWallWave.ts`、`BravaisTile.tsx`）：首页层之间换页签（`replace`）是整墙波次：变了内容的磁贴按离视口左上角的对角线格数错开（30ms/格，上限 340ms），远的先抬起 90 世界单位、缩到 0.88、淡出（280ms `cubic-bezier(.4,0,1,1)`），全部出场（620ms）后近的先落回（420ms `cubic-bezier(.2,.9,.3,1.04)`）。点集合进去（`push`）从被点的磁贴翻牌，返回、数据到达从缝的中点翻牌；波次进行中到达的数据按波次的时间表落回（`mergeFlipSteps`）。前后同一位置同一条目的磁贴不动。换层前在屏上的磁贴存成 `Ghost` 画出场，`Wall::capture` 由 `library.rs` 的 `rebuild` 在换数据前调用。
- 墙的平移（`wall_pan.rs`，取数自 `wallPanMotion.ts`、`useWallPointerPan.ts`）：按下即停掉惯性和滚轮滑行；移动超过 7px（`DRAG_THRESHOLD_PX`）才开始拖，从按下的位置算起；松手速度低于 40px/s 或最后一次移动早于 80ms 不滑行，否则相机滑 `0.3 × min(速度, 4000)` 屏幕像素、时间常数 280ms、剩 0.5px 停。鼠标滚轮（winit 按行报告，一格按 Chromium 的 100px，Linux 53px）累加进待走距离，每帧按 80ms 时间常数追上去，单帧最多按 50ms 算；触控板（像素增量，两轴同时有或单轴小于 50px，同一手势 120ms 内一旦判成触控板就一直是）当帧直接走并接过还没走完的滑行；Shift + 竖向滚轮改横向。
- 首页封面墙：格子 128px、间距 8px，12×8 格的块，4 个模板 × 4 种镜像（`mixBlockCoords` 用 32 位乘法照搬），槽位按 `|dx| + |dy|·0.9 − √面积·136·0.35` 排序后依次放曲目；整体缩放 <640px 0.52、<1100px 0.64、其余 0.76。层次自下而上：`Wash`（底色 + 左上 accent 16%、右下 secondary 20% 两团光晕）、墙面 canvas、`Veil`（暗角 52%/日间 16%，静态颗粒）、右上工具按钮。磁贴：底色为背景混 12%（日间 6%）文字色，封面按 `object-fit: cover` 在解码时裁成磁贴比例（`folia_library::cover_for`，只缩不放），自上而下 5%→5%(34%)→24%(56%)→92% 的黑（日间白）渐变（`Shade`：画成 1×128 的图片，iced 同一个 canvas 里网格总在图片之下，填色渐变会被封面盖住；角标底同理用 1×1 图片），左上 14px 编号角标，左下 17px 标题（`clamp(22px, 2vw, 36px)`，行高 0.96，最多 3 行）与歌手（13px，单行），正在播放加 2px accent 描边。
- 墙面性能：canvas 自己发 `Viewport` 消息上报尺寸；排版范围是视口外再加 `OVERSCAN` 420 世界单位（`BRAVAIS_OVERSCAN`），相机走出 420 − 180（`CULL_EDGE_MARGIN_PX`）才重新裁剪、给新进来的磁贴断行和取封面，所以封面在磁贴进视口前就开始加载；绘制只画视口内的。标题断行与省略按（曲目, 宽度）缓存，封面缓存按字节淘汰（见下），换层不清空。封面没到时画海报底色、遮罩、角标和文字（原版 `background-color` 加上面各层），到了一帧内换上（CSS 背景图没有过渡）；只有没有封面地址的条目画 `fallbackBackground` 色相渐变（本地文件读不到封面也算没有地址）。绘制全部用窗口坐标：iced 0.14 的 `Frame::with_clip` 不会把图片和文字平移进裁剪区。
- 首页外壳（`home/`，取数自 `bravais.css` 的缝、`bravaisHome.css`、`BravaisSeamHomeTools`、`WallToolsButton.css`、`SlideActionButton`、`WallBackButton.css`、`bravaisSeamMotion`）：窄缝宽 120、书脊 64，开合 0.36s `cubic-bezier(0.65, 0, 0.35, 1)`。缝纸色是主题背景 90% 混文字色 10%。次要文字是文字色 55%。阴影是 1px 文字色 6%（日间 8%）的描边，加 `0 24px 64px` 黑 42%（日间 `rgb(42 35 27 / 18%)`）。页头「书库」居中，15px、字重 800、行高 1.3、字距和首行缩进 0.3em（书脊 12px、0.08em），字距用逐字间距摆。窄缝内边距 16×10×0、段间距 18；书脊 14×8×0、段间距 12（`bravaisHome.css` 盖过 `bravais.css` 的 22×10 和 16×8）。页签见下面「首页页签」。折叠钮是 `FoldHorizontal`，36px、圆角 12，把缝收成边缘标签，不是在窄缝和书脊之间切换。书脊底部另有 `Maximize2` 回到窄缝。工具格窄缝两列 40px、间距 8，书脊一列 36px、间距 6，图标 18px，平时透明；队列格禁用、透明度 0.3。⋯ 菜单比工具格左右各宽 6px，底边在工具格上方 8px，内边距 4，圆角 12，弹出 0.18s `cubic-bezier(0.2, 0.7, 0.25, 1)`（收起 0.12s ease-in）。右下按钮 48px 圆，距右下 24px（窗口宽 <640 时 16px），深色底黑 40%、浅色白 70%，图标 Settings2 20px，打开后是 X，`shadow-lg`。面板在按钮上方：`.lattice-tools-stack` 的 `bottom` 是 64px，按钮高 48px，所以面板底边离按钮顶 16px。面板宽 `min(296, 窗口宽−32)`，高由快捷排和音量行决定，内边距 10，快捷排下边距 4 再加面板行距 2。1px 白 8% 描边（日间黑 7%），圆角 24，底色黑 40%（日间白 62%）。快捷排 6 列、间距 4、格高 40、圆角 12、白 6% 底（悬停 13%；日间黑 5%/10%），图标 18px。画出的格是洗牌、主题、队列、播放器、设置；去处（Lattice）那一列空着。洗牌、主题、队列禁用、透明度 0.35；没有歌时播放器也禁用。音量行最小高 38，间距 8，内边距 0 12 0 4，圆角 12，白 6% 底（日间黑 5%）。左侧 30px 图标按钮圆角 9、图标 17px，点击静音并记住原音量。滑条轨道 4px，已填充部分是主题强调色，其余是文字色 18%。拇指 14px 文字色，1px 黑 22% 描边。右侧数值宽 38、12px、字重 600、透明度 0.66。圆角底和阴影由 `folia-render` 的 `Plate` 画，阴影在控件边缘衰减到 0。左上返回 40px 圆，顶 20（窗宽 <640 为 14）、左 24（窄窗 16），相对窗口。热区 120px，显隐 200ms CSS ease，隐藏时左移 6px 并把直径收到 0.92。返回画在缝下面。缝内容底边距：没有歌 20px；有歌时弹簧（刚度 280、阻尼 28）过渡到 `底栏偏移 + 88`，默认 120px，工具格因此停在收起的播放条上方。横缝把这个底边距清掉。点面板或菜单外面，或按 Esc，收起。
- 去掉早期顶栏和底栏之后各项的去处：

| 原来的功能 | 现在的位置 |
|---|---|
| 回到首页 | 首页就是墙。搜索浮层的 X 和 Esc 回到首页。播放器面板封面页的房子图标关掉面板并回到首页 |
| 搜索 | 缝的搜索格，打开全屏搜索浮层。不在缝里先展开搜索框 |
| 歌词动画 / 正在播放 | 工具面板快捷格「播放器」（没有歌时禁用）；缝「⋯」里的「回到播放页」（有歌才出现）；左上角返回（有歌且指针在热区）。播放器面板控制页可以左右切换已移植的样式 |
| 设置 | 缝的设置格、工具面板的设置格，以及播放器面板封面页的设置图标 |
| 播放 / 暂停、上一首、下一首、进度、循环 | 正在播放页的胶囊。循环也在播放器面板控制页，点一下在关闭、列表、单曲之间转 |
| 音量 | 工具面板音量行，以及播放器面板控制页。静音记住原来的音量 |
| 导入文件夹、重新加载 | 缝「⋯」菜单 |
| 播放队列 | 播放器面板的播放列表页：点歌名跳过去播，也可以下一首播放、移到队尾、删除。缝和工具面板的队列格仍禁用。没有洗牌 |

- 正在播放页右下角的播放器面板（`panel.rs`，取数自 `PlayerPanel.tsx`、`UnifiedPanel.tsx`、`panelTab/shell/`、`panelTab/` 各页）。只在正在播放页挂上。宽 360、最大为窗口宽减 32（`PanelShell.tsx:100` 的 `w-[360px] max-w-[calc(100vw-2rem)]`），圆角 20，内边距 12（同文件 100、111 行）。右边距窗口宽 ≥768 时 32、否则 16（`pr-4 md:pr-8`，158 行）。底边在胶囊基线之上：胶囊底 32（`playerBottomBarLayout.ts:6`），再留把手高 48 和 `HANDLE_ROOM` 60（`PanelShell.tsx:20`）。顶上至少留 `TOP_MARGIN` 56（24 行）。页签条高 `DOCK_HEIGHT` 36、与正文间隔 `DOCK_GAP` 12（26–27 行）。正文顶上是 gap 12、1px 线、under 8（29 行）。底色 `--pn-glass`：深色 `rgba(28,28,31,0.8)`，浅色 `rgba(244,244,245,0.8)`（`panelTheme.ts:21`）。阴影 `0 30px 80px` 深色黑 0.55、浅色黑 0.24，再加 1px 环深色白 0.09、浅色黑 0.06（22–24 行），由 `Plate` 画。颜色用 `settings_kit::Tokens`；深色 `--st-raise` 改成白 0.18（`panelTheme.ts:20`），不改设置窗口那套灰。喜欢的心是深色 `#f2607a`、浅色 `#e0354f`（25 行）；现在没有「已喜欢」的状态，心是灰的。封面页大封面圆角 10、没有封面时 Disc 40（`SongHeader.tsx:54–60`），歌名 18px Semibold、歌手 13、专辑 12，封面和文字间隔 14（19 行）。按钮行上边距 14、间距 6（`SongActions.tsx:32` 的 `mt-3.5 gap-1.5`）：心、星（有歌才画）、房子、设置，图标 17，按钮 32、圆角 8（`PanelIconButton.tsx:10–11`）。折到别的页时头高 48（`SongHeader.tsx:17`），小封面圆角 8、Disc 20，歌名 14、歌手 12，右侧一颗心；点这一行回到封面页。页签顺序跟 `UnifiedPanel.tsx:252–267`：本地是封面、本地、控制、播放列表、音质；在线把第二格换成歌词。音质的越南文是「Tài khoản」（`vi.ts` 的 `panel.account`），中文是「音质」。页签条高 36、内边距 2、圆角 10，选中块圆角 8、底色是 raise，字 13（`Segments.tsx` 的 l 档）。打开 300ms、关掉 220ms，曲线 `(0.22, 1, 0.36, 1)`（`motion.ts:16–18` 的 unfold / move，`KIT_EASE.out` 33 行）。把手 48px 圆（`PanelHandle.tsx:399–402`），关着是 Settings2 20、深色黑 40% 白图标、浅色白 70%，开着是白底黑 X；进出 240ms ease-out，藏起时右移 20、下移 12（317–323 行）。平时一直显示，播放页控件闲置（`chrome_visible` 为假）时用同一段 240ms 收起。开着是白底黑 X。能用的：音量与静音、循环、已移植歌词样式的左右切换、浅色/深色、队列点歌名跳转、下一首播放、移到队尾、删除、房子回首页、设置图标打开设置。封面没有图时用 Disc。本地页写歌词来源（无 / 本地 / 在线）、时间轴 0 ms、在线匹配、ReplayGain 未检测到。音质页是游客模式和「去主页登录」，默认音质显示「标准」。
- 顶栏（`chrome.rs` 的 `Titlebar`，取数自 `AppShell.tsx:34,145–166,180–226`、`WindowControls.tsx`、`TitlebarDragZone.tsx`）：顶上 32px 整条可拖动；指针在窗口顶部 56px 内时出现全屏、最小化、最大化、关闭四个 44×32 按钮，从透明、上移 4px 过渡到 0.75（浅色 0.85），悬停到 1 并加底色（深色白 10%、浅色黑 5%），关闭键悬停红 `#ef4444` 白字，过渡 200ms Tailwind 曲线。底板按默认设置：播放页没有；其他页浅色 `white/12` 加底边 `white/15`，深色只有模糊（不做），所以看不见；淡入淡出 0.18s easeOut。只有全屏和最大化有提示。遥控按钮与点击穿透锁没有后端，不画。
- 正在播放页底部播放控制条（`player_bar.rs` 的 `PlayerBar`，取数自 `FloatingPlayerControls.tsx`、`floating-player/`、`ProgressBar.tsx`、`useTrackTitlePreview.ts`）：行宽 `min(512, 窗口宽)` 减 32；收起时宽为行宽 60%（窗宽 <768 为 80%）、高 48，只有进度条；指针进入行（含下方 32px 命中区）20ms 后展开，离开 100ms 后收起，有歌且暂停时保持展开；尺寸用弹簧（刚度 280、阻尼 24）。展开高 72（<640 为 127，标题、按钮、进度三行）：左侧 48px 实心播放 / 暂停（悬停放大 1.05），中间标题 14px 粗体（悬停出现上一首 / 下一首箭头，靠近一侧时标题淡出、显示相邻曲目 55%，切歌后 1.1s 不预览，换歌时新标题从切歌方向滑入 14px），下方 `mm:ss`（10px 等宽、次要色 60%）、6px 进度条、总时长；右侧两个槽位：循环（三态图标，开启时高亮）、歌词时间轴（未移植，禁用 20%）。底色深色黑 20%→40%、浅色白 40%→60%，过渡 300ms；阴影深色 `shadow-2xl`，浅色展开 `shadow-xl`、收起 `shadow-lg`（取第一层），由 `Plate` 画。控件闲置时 0.26s easeOut 下移 24px、缩到 0.97 并淡出。首页不放控制条。
- 面板开合：打开 300ms、关闭 220ms，曲线 `(0.22,1,0.36,1)`，透明度与 0.92 缩放，以把手中心为原点（`float` 平移加缩放）。面板底边在窗口底 92px（基线 32 加把手位 60），控件闲置把手藏起时降到 40px（`BARE_GAP_PX` 8）。阴影是 `0 30px 80px` 高斯加 1px 外环。把手：关着深色黑 40% 白图标、浅色白 70% `#18181b` 图标，开着白底黑 X，颜色过渡 300ms；宽 ≥768 悬停放大 1.05，窄窗半藏 24px、60% 透明，悬停复位；进出 240ms easeOut 带 x20、y12、0.92 缩放。
- 控制页外观行（`mode_picker.rs`，取数自 `ModeStepperRow.tsx`、`QuickEffectPicker.tsx`、`PictureGrid.tsx`、`Popover.tsx`、`QuickControlChip.tsx`）：`‹ 字形 说明/模式⌄ 强度 ›`，箭头 28px 直接换相邻模式（换后字形和名称从箭头方向滑入 10px），点中间 48px 那块打开选择器，强度胶囊 28px 在右箭头前。选择器在面板内、行下方（放不下时在上方）左右各 8px，三列 16:10 小图、间距 12、最高 300 可滚，底部「更多设置」打开设置；小图是真实样式在 1200×800 舞台上画示例歌词再缩小，静止停在 `posterTime` 6.0s，指针停在哪张哪张播放。照原版 `pictureImages.ts` / `frozenPicture.ts`，静止的小图只画一次：`folia_visualizer::Pictures` 按「样式 + 主题 + 字号 + 时刻」保存一份 iced 画布缓存，最多 64 份，最旧的先删；缓存在 `Pickers` 里，整个运行期间都在，关掉选择器再打开不会重画。`Frozen` 用保存的缓存显示，只在尺寸变化时重画；指针停着的那张用 `Scene` 实时画。随机时下面一行是本曲抽到的样式和随机池两个胶囊（无后端，禁用）。背景只有「通用」一种，箭头 40% 禁用。
- 面板头部（`SongHeader.tsx`）：封面页是整宽大封面加歌名、按钮；换到其他页时 300ms（`st-t-unfold`）把封面从整宽缩到 48px、圆角 10 到 8，头部高度跟着缩到 48，正文高度同时展开；折叠后的歌名和心 220ms 从上方 4px 淡入，大歌名和按钮 120ms 向下 8px 淡出。
- 歌词页（`SourceTab.tsx`）：歌词来源（在线歌为「网易云音乐」）、时间轴偏移步进器（±250ms，非 0 时出现重置；运行时按歌生效，换歌归零，不写入设置）、在线匹配与「导入 / 导出歌词」两个按钮（无后端，禁用）、音频增益四段（只有「关闭」可用）。
- 首页也挂控制条（`buildAppOverlaysModel.ts:255`）：行宽 `min(512, 窗口宽 − 120)`，只在悬停时展开，点胶囊空白处进播放页。
- 控制条离底高度：`folia_store::PlayerSettings`（`player.toml`，`bottom_bar_offset`，默认 32，上限 `窗口高 / 2 − 132`）。设置「播放页 › 控制栏」有高度滑块、恢复默认和「在播放页拖动调整」。拖动模式（`positioner.rs`）：关设置、进播放页，控制条强制展开，虚线框标出可放的范围，顶部胶囊有提示、重置、取消、保存，拖控制条上下移动，Esc 取消、Enter 保存；面板、把手、字幕（偏移 + 80）都跟着基线走。
- 书库缝（`bravaisSeamLevel.ts`、`seamPlan.ts`、`useBravaisCamera.ts`）：等级 `full` 是首页窄缝 120px（横条 72），`spine` 是书脊 64px（横条 52），`hidden` 是 0 并在窗口边留标签。宽 <900 默认书脊，跨过 900 回到该宽度的默认。开口 0.36s `cubic-bezier(0.65, 0, 0.35, 1)`。内容透明度 `(开口 / 排版宽度 − 0.35) / 0.65`。缝用 `blockSeamPlan` 开在相机附近的块边界上（原点相机得到世界 x −4），跟着相机走，离边 14px 内收窄，收到看不见就出边缘标签；点标签在当前视口另开。首页浏览是无限墙，拖动不阻尼，松手按 `0.3 × 速度`、时间常数 280ms 滑行。高 ≥ 宽 × 1.1 时默认改成横条，先合上再在块行边界张开。空库墙上不写「暂无本地音乐」，写 `libraryBravaisHome.wallHint.localTitle` 和正文，画在缝较宽的一侧；「暂无本地音乐」只在缝的状态行。
- 拖动区和窗口按钮用默认箭头光标（原版 `-webkit-app-region: drag` 不改光标）。上一首不再「超过 3 秒先回到开头」（原版 `handlePrevTrack` 直接切上一首）。
- 静音：`Playback::muted` 一份状态，首页工具面板和播放器面板共用；静音不改 `volume`，取消回到原值，拖动音量即取消静音。
- 封面缓存：`folia_library::CoverCache` 按字节淘汰最久未用的。墙面 48MB（1080p 一屏约 14MB，留三屏），按显示尺寸（乘墙面缩放）解码；面板 4MB（360px 一张约 0.5MB，留 8 张）。
- 全屏着色器通道（`folia-render::pipeline::Fullscreen`）每个图元一份 uniform：iced 先准备一帧所有图元再统一绘制，共用一份缓冲时同一帧里的多个 `Plate` 都按最后一个的参数画，首页打开工具面板时缝的位置画出了面板的白色圆角块。
- 播放器面板与原版的差异：表面和把手的 `backdrop-blur` 没做，只留底色透明度。热区导轨（宽 96、左端 Command 14px，`PanelHandle.tsx:336–374`）和沿导轨滑开命令面板没画：还没有命令面板。把手平时一直显示，只在播放页控件闲置时藏起（`usePlayerPanelModel.ts:126` 的 `hideToggleButton`）。`resolveLikeAvailability`（`playerLikeAvailability.ts:18–44`）对本地歌和网易云（`likes: true`，`omni.ts:718`）返回可用；这里没有喜欢的存储，心禁用，悬停是 `player.like`，不用「暂不支持喜欢」。星在本地和网易云都 `isApplicable` 且 `canAdd`（`AddToPlaylistHost.tsx:51–54`，网易云能新建歌单）；没有歌单选择，星禁用，悬停是 `localMusic.addToPlaylist`。洗牌按钮照 `QueueTab.tsx:260` 画着但禁用：不足两首时悬停 `wallTools.shuffleUnavailable`，两首及以上悬停按钮自己的 `queue.shuffle`，`Queue` 没有 shuffle。时间轴偏移没有步进（没有偏移字段），停在 0 ms。ReplayGain 只有「未检测到」一句，不是四段选择。在线匹配不能点。音质不是下拉，只显示禁用的「标准」；网易云未列出自己的档时原版是 standard / high / lossless / hires（`omni.ts:88–91`）。不画登录窗、同步、HiFi 信号路径、FM / Navidrome / Folium 页、队列墙、保存为歌单。智能过渡和音频效果器两块禁用。小图是圆角 8 的画框里的直角 canvas：iced 不能把 canvas 裁成圆角。拖动模式的虚线框和胶囊没有 0.2s / 0.22s 的淡入。背景模式没有存储，停在「通用」且不能换。透明背景没有设置项，开关停在关。随机样式时「换一种」和「随机池」禁用：随机只按曲目 id 取一次（`registry.rs` 的 `random_for`），没有可改的池。AI、自定义和快速编辑没有编辑器，来源停在「默认」，另外两格和调色板禁用。生成 AI 主题禁用。同步按钮只在来源不是默认时出现（`ThemeNameRow.tsx:41`），这里不画。动画强度能在这次运行里循环，不写入 `appearance.toml`。页签太窄时原版改成 16px 图标，这里仍写文字。正文高度按内容估算，不是量出来的。队列标签 `getSongTag`（`songAvailability.ts:33–36`）只在下架或 MV 时出现，普通的本地歌和在线歌没有「本地 / 在线」标签。封面解码在后台，当前这一张 360×360 RGBA 约 0.5 MB，换歌就丢掉上一张。
- 首页还没做、也不算已接受的差异：块底板、空画框、正在播放大格子的进度条、悬停动效、标题字距 −0.045em。没有后端所以不画空壳：Navidrome、文件夹/专辑/歌手/歌单二级切换、扫描进度、直达入口、舞台。工具面板不画墙面外观、透光、叠色、开灯/关灯、帮助。队列不支持洗牌，洗牌格禁用。没有从封面生成主题，主题格禁用。没有 Lattice，去处那一列空着，队列格禁用。这些原版有、这里没有，所以不画，也不另做替代。工具按钮、工具面板、左上角返回、搜索浮层原版都有 `backdrop-filter`，这里只保留底色透明度，不做模糊，也不做近似模糊。面板打开的 0.9 缩放和菜单的 0.94 缩放没做，只做透明度和菜单上移 8px（`float` 可以缩放，未改）。返回的 0.92 缩放改成按钮直径从 36.8 收到 40。音量拇指没有 `0 2px 6px` 那一层阴影，按下也不放大到 1.18。竖排和字距是逐字摆，不是 `writing-mode` / `letter-spacing`。工具按钮不支持滑开命令面板。搜索浮层的 motion 没有写时长，这里用 200ms ease-out 做透明度和下移 28px。结果列表仍是 `track_list.rs`，样式留到第 7 项。首页页签按 `bravaisHome.css`：内边距 9×4、圆角 10、字距 0.18em。返回的 `left` 是窗口的 24px。缝内容绕 Y 轴翻转（160ms + 240ms）没做：iced 没有子树的透视旋转，开口变窄时只按 `contentOpacity` 淡出。空墙说明的字距 −0.01em 没做：说明要按宽度折行，不能拆成单字。面板投影的着色器比圆角底四周多出 blur 64、下方再加下移 24，阴影在这个范围内收到 0；右侧超出窗口的部分由窗口裁掉。⋯ 菜单没有 `0 12px 32px` 黑 34% 的投影，只有 1px 描边。搜索进行中的图标是静止的 Loader，不旋转。
- 首页页签（`library.rs` 的 `TABS`，`home/seam.rs` 的 `tab_column`）：顺序照原版实际渲染页签的 `resolveHomeTabs`（`homeSources.ts:110`）：歌单、电台、探索、专辑、本地，文案 `home.playlists`、`home.radio`、`home.explore`、`home.albums`、`localMusic.folder`。`homeSources.ts:24` 的 `ONLINE_TABS` 只是「哪些算在线页签」的集合，不决定顺序。网易云声明了全部能力（`neteaseProvider.ts:302`），所以五个页签都可用，探索页签出现。竖排、逐字摆：14px、字重 700、字距 0.18em、内边距 9×4、圆角 10；未选中文字色 55%，选中文字色并填 14%；悬停 1px 26% 描边、16% 底（书脊 12px、0.1em、7×3）。纵向放不下时照 `useBravaisSeamTabsFit` 的顺序退让：先隐藏「书库」，再把页签缩成一个字（`abbreviateSeamTabLabels`：首字，撞车的取两个字；内边距 5×4、字距 0），按窗口高度算（`fit`）。悬停显示全名。横的缝里页签横排。当前页签存进 `general.toml` 的 `home_tab`，下次启动打开它。⋯ 菜单里的「导入文件夹」「刷新」只在本地页签出现（原版这两项属于本地页签），「回到播放页」是应用项，两组之间一道 1px 14% 分隔线；没有任何项时 ⋯ 不弹菜单。
- 在线状态（`online.rs`）：启动时在后台线程读密钥库里的会话，有就调 `account`；读到账户即已登录；读到没人、且会话是从密钥库恢复的，就是「登录已过期」（`needsRelogin`），同时删掉这份会话（`signInRanOut`）。歌单在读到账户时就读；专辑、电台、探索第一次切到对应页签才读（`attachOnlineHomeFeeds`），换账户全部作废，晚到的回答按账户 id 丢掉。头像下载后按 64px 解码。
- 在线墙：未登录时在线页签的墙是空的，画访客说明（`onlineWallHint`）：标题 `libraryBravaisHome.wallHint.guestTitle`，正文 `guestBodyWithLocal`（`connect` 是 `libraryBravaisHome.connect`，`local` 是本地页签名）；登录过期时是 `reloginTitle` / `reloginBody`，状态行 `status.loginExpired`。已登录：歌单页签铺用户创建与收藏的歌单（描述是创建者，没有就 `home.playlists`）；专辑页签铺收藏的专辑（描述是歌手，没有就 `player.unknownArtist`）；电台页签铺私人 FM、每日推荐、推荐歌单（`buildOnlineRadioCards`）。磁贴照 `projectHomeCardItem`：集合的曲目数并进左上角类型标签（「歌单 · 124」，`libraryBravais.kind.*`），私人 FM 只写「电台」。墙仍是 `wall.rs`：`Tile` 泛化为本地 / 在线封面（`CoverSource::Local` 读文件，`Remote` 经 `Omni::fetch_cover` 下载，再在线程里按磁贴尺寸裁切解码）、可选文字标签、探索标题磁贴；`Shape` 选择排法（`Ranked` 原有的按距离排槽，`Explore` 探索地块）。状态行照 `BravaisHomeDirectory`：有磁贴不说；读取中 `playlist.loading`；读完为空 `home.loadingLibrary`；账户解析中 `home.loadingLibrary`。
- 点在线磁贴：歌单、专辑、每日推荐打开集合层；私人 FM 直接播放读到的 FM 歌曲并在快放完时补歌（`isPersonalFmCard`）；探索里的歌先展开成聚焦卡，卡上「立即播放」连同所在那一组的歌一起放；探索里的集合打开集合层。
- 集合层：墙换成这张集合的曲目（编号角标），缝从 120 展开成 300 的完整信息条（`BRAVAIS_SEAM_FULL_WIDTH`；横的缝 212），照 `BravaisSeamContent` 的 `FullSeam`：‹ 返回（32px 圆，白 8% / 日间黑 5%）与面包屑「书库 › 名字」（11px，文字色 55%）、上下引号之间的竖排标题（字号 `verticalTitleSize(title, 52, 26)`，字重 800）、1px 12% 分隔线、元数据「歌单 · N 首」（`libraryBravais.trackCount_one/_other`）、状态行、「播放全部」（44px 高，15px 粗体，16% 底）。书脊（64px）照 `SpineSeam`：返回、折叠、竖排标题（`verticalTitleSize(title, 30, 18)`，点它展开）、竖排计数、播放、展开。点曲目磁贴从那一首起放这张集合。
- 字重（`typeface.rs`）：界面所有字重都经 `typeface::weight` / `font` / `with` 一处映射：800、900 用 700，500 用 400。Windows 的 Segoe UI 没有 800，cosmic-text 不合成字重，找不到 800 时会退到别的字体（衬线）；Linux 上验证过，Windows 未验证。
- 在线墙铺满整面（`wall.rs`）：照 `getInfiniteSlotItem` / `getWrapPeriod`，条目按块（12 个座位）铺，周期取整块，起点按 `FIELD_ASPECT 2.2` 旋转，所以条目少时也重复铺满，不留空。过滤时换成有限的排名墙（`Shape::Finite`）。没有封面的条目画 `fallbackBackground`：按卡片 key 的字码和取色相，145° 从 `hsl(h 68% 58%)` 到 `hsl(h+52 62% 18%)`；歌单与专辑卡右下有两层叠页边。
- 集合层（续）：「播放全部」旁有「加入队列」（`enqueueAll`，追加到队尾，已在队列的不重复）、星标（收藏歌单 / 专辑，`options.subscribe*`，自己创建的歌单不显示）、「列表」与「⋯ 更多」（重新加载、折叠信息条）。面包屑下是「过滤当前页」（`BravaisSeamFilterField`：漏斗、下划线输入框、匹配数、清除），按标题、歌手、专辑过滤，墙换成有限墙，放歌以过滤后的歌为队列。「列表」打开 420 宽的列表面板（`BravaisListPanel`：面包屑「… › 列表」、44px 行、编号 / 标题 / 歌手 / 时长，正在播放的编号用强调色，单击定位到墙上的那一格，双击播放）。
- 登录窗口（续）：平台列外面留 8px 内边距，选中块与 3px 色条不再贴边，描边是最上层的 1px 外环。失败后消息下有「详情」（`LoginDetails`）：平台、环节（`loginStepText`）、类别与情况 id（`LOGIN_KIND_TEXTS`）、原始信息（错误链或平台码），窗口按详情加高 140。「稍后再试」（HTTP 429）先倒数 20 秒（`DEFAULT_RETRY_WAIT_MS`，门面拿不到 `Retry-After`），按钮底色每秒往前长一格，数完才变成「刷新二维码」，倒数期间重来被挡住。
- 「定位」（播放器面板「正在播放自」药丸）：首页记住队列从哪面墙开始（本地 / 探索页签，或某个集合）；定位时回到那面墙，集合层没开就重新打开、曲目到了再定位，把相机移到这首歌离得最近的那一格（`useBravaisLocate`）。私人 FM 没有自己的墙，不定位。
- 播放器面板的「播放列表」（`queue_island.rs`，取数自 `WallToolsQueueIsland.tsx`、`WallToolsQueueRow.tsx`、`queueDragGeometry.ts`、`useQueueDragReorder.ts`）：40px 一行（手柄 18×32、3px 标记、12.5px 标题与 10.5px 歌手，悬停出现下一首播放 / 移到末尾 / 移除，正在播放的那行只有移到末尾），列表最多 8 行、至少 74 高，滚动条用 `settings_kit` 的 `scrollbar` / `scroll_style`。虚拟列表：只构建视口上下各 4 行（`overscanCount`）之内的行，每行用 `pin` 放在自己的 y 上。打开时把当前歌滚到正中（只做一次，等列表上屏后的下一帧）；换歌时按 react-window 的 `smart` 平滑滚到当前行（已在视口内不动、离得远则居中，拖动中不滚），平滑滚动 300ms。按住手柄拖动排序：超过 4px 才开始，幽灵行（白 18% / 日间 92%、`0 10 28` 阴影与 1px 环）跟着指针，其余行 140ms 让开（`queueRowShift`），指针离上下边缘 32px 内自动滚动（最快 14px/帧，起点所在那一侧离开后才启用），滚轮照常滚、落点跟着变，松手时被拖的那首还在原位才提交，Esc 取消。指针在列表外也能拖：`drag_capture.rs` 的 `Capture` 包住列表，按下手柄后接管全部指针移动与松开（`setPointerCapture`）。顺序变化走 `Queue::move_to`。
- 专辑海报（`poster.rs` 与 `home/seam.rs` 的 `album_poster`，取数自 `BravaisSeamAlbumPoster.tsx`、`bravaisAlbumPoster.css`、`bravaisPoster.css`、`bravaisPosterComposition.ts`、`bravaisPosterPalette.ts`、`temperaPalette.ts`、`colorPalette.ts`、`bravaisAlbumModel.ts`）：专辑集合层在完整等级、没开列表和过滤时，缝变成 `resolvePosterWidth` 宽（视口 46%，520–680）的海报。颜色：封面按 128px 取、缩到 50px，加权中位切分取 2 个与 5 个代表色（`extractRepresentativeColorsFromPixels`），经凝彩的渐变色阶（`TEMPERA_TONE_STOPS`、`THEME_HUE_MIX`、`matchLuminance`）得出纸、墨、色块、标题、四档立体色与强调色（`resolveBravaisAlbumPalette`）；取不到时用 CSS 的米色、藏青与橙红。构图按集合 key 用 FNV-1a 与 mulberry32 定死（斜线两端 44–50 / 56–62%、封面 ±2.5–6°、胶带、下划线 38–62%、水印在哪边）。底（一次画进 canvas 缓存，尺寸或颜色变了才重画）：色块、斜线以上的点阵纸（9px 一个 1px 墨点）、斜线下 3.2% 高的 −58° 网纹、右下渐隐的纸色点阵、水印字。界面：返回与面包屑（墨色）和「过滤当前页」（1.5px 墨框，点了换成带过滤框的普通信息条并聚焦输入框）；斜贴的封面（512px）带 12px 墨色硬阴影和胶带，右上竖排碎字；「专辑 · 年份」小标签（年份取发行时间，按 UTC）；奶油色大标题（`min(150, 15%高, (宽−80)/(列数+1.4))`，字重 900，四层 0.025em 立体残影，最长的词 0.72 倍）；−1° 的强调色下划线；歌手（19px 带下划线）· 首数；简介一行；状态行；46px 方角按钮带 5px 墨色硬阴影（播放全部是强调色），星标与「⋯ 更多」。
- 专辑海报的入场（`poster_motion.rs`，取数自 `bravaisAlbumPoster.css` 的 keyframes）：每次海报挂载（打开专辑、从列表或过滤回来）放一次。纸的下沿 620ms `(.65,0,.25,1)` 从底边收到斜线；120ms 起封面 560ms `(.2,.9,.3,1.08)` 从上方 28px、1.14 倍落下（透明度在前 60% 走完）；300ms 起标题每个词错开 70ms、520ms `(.2,.9,.3,1.1)` 从三个方向之一（按第几个子元素）带转角拼上；420ms 起小标签上浮；480ms 网纹、520ms 点阵与水印淡入；560/620/680/740ms 起下方各块上浮；620ms 下划线从左拉出；640ms 起立体残影 420ms 长出；700ms 碎字、820ms 胶带 `(.3,.7,.3,1.2)` 从 1.7 倍拍上。标题改在 canvas 里排（按列宽折行、整词换行，太长的词按字断开），所以放大的那个词也照构图错开并旋转。入场期间底板每帧重画，放完回到缓存。
- 封面飞行（`cover_flight.rs`，取数自 `bravaisCoverFlight.ts`）：从墙上点专辑时，封面从被点的那张磁贴飞到海报封面（640ms，三次 ease-in-out，途中放大 5%，墨色硬阴影在后半段从 0 长到 12px），海报自己的封面先藏着、落地同一帧露出，且不再放「落下」的入场；终点每帧按海报封面 canvas 报来的位置量，600ms 内等不到就就地 150ms 淡掉。从专辑返回首页时飞回那张专辑的磁贴（560ms），落地后在磁贴上 200ms 淡掉。
- 专辑墙（`album_wall.rs`，取数自 `bravaisAlbumWall.ts` 与 `bravaisAlbumPoster.css`）：海报在场时，曲目都用同一张封面就按块铺拼贴（每块一张边长为块长边的封面，居中，磁贴各露出自己那一格；大图 1024 档到之前用海报的 512 档垫着；底部只压一道淡的暗带，文字下垫一层阴影），有曲目带自己的封面时首次出现的画原封面、其余画碎片（50×50 读像素按亮度梯度挑 6 处细节，按曲目键选一处放大 2–3.5 倍；海报双色调与网点预先烘焙进图；正在播放和键盘焦点那张还原）。
- 整层倾斜（`useBravaisPosterTilt`）：竖向的缝里是海报时，两半墙绕海报右缘、窗高一半处转 −10°（120ms 后 950ms 转入，回正 500ms，`[0.45,0,0.2,1]`）；海报的底在墙的 canvas 里跟着转，上下各多画 40% 窗高的纸与色块盖住转出来的角；点击按转回去的位置判定；键盘与聚焦卡的相机让位按转后的外接框反复修正（`bravaisPosterReveal`）。
- 聚焦卡（`focus_card.rs`、`block_reflows.rs`，取数自 `useBravaisFocus.ts`、`BravaisFocusCardBody.tsx`、`bravaisReflowMotion.ts`、`wallReflowMotion.ts`、`revealRect.ts`、`bravaisKeyboardModel.ts`、`blockReflows.ts`、`lotSeats.ts`）：点一首歌（本地、集合、探索）不再直接播放，而是在块里就地长到 6×6，块内其余 11 张按原版生成的让位表（`block_reflows.rs` 由 `blockReflows.ts` 生成，探索的地按 `cutGrownLot` 求移动最少的座位）换位，块外不动。让位走 stiffness 300、damping 34 的弹簧，按块宽采样成折线缓动，时长取 framer 走到静止的时刻；收起反向，换到别的块时旧块同时收回。相机按 `resolveRevealCenter` 做最小平移（420ms，`(0.22,1,0.36,1)`），能让缝留在屏内时优先。卡片正文：大标题（clamp(48, 6vw, 82)，行高 0.95，最宽 76%）、歌手 · 专辑 · 时长、立即播放（正在播放时是暂停 / 继续，旁边多一个进入播放页）、加入队列（已在队列时是「已在队列」，悬停提示「插入队列」，点了挪到队尾）；按钮排 140ms 后 240ms 从下方 12px 淡入。键盘：方向键移焦点（发丝线、3px 墨色环、发丝线，四边外扩一个间隙），Enter 展开、再按播放，Shift+Enter 加入队列，Home / End（有限墙）首尾，PgUp / PgDn 翻 85% 屏，Esc 依次收起聚焦卡、去掉键盘焦点、关列表、清过滤词、返回。过滤框里 Esc 先清词、空了再收起回到海报，Enter 收起并把焦点交给墙。
- 探索页（`library.rs` 的 `compose_explore`，`lots.rs`）：数据是网易云探索的第一页（`neteaseExplore.ts:468` 的 `firstPage`）：已登录时每日推荐歌曲在最前，然后是平台首页的块（去掉排行榜块与听歌记录块，每块最多 12 个），再是每日推荐歌单、排行榜（各最多 12 个，排行榜第二行是更新频率）。墙照原版探索拼贴排：一组占一块 12×8 格的地（`lotTables.ts`），超过 13 个分到相邻几块，地里按种类排（专辑、歌单、歌曲）、大座位在前，标题磁贴占左上 6×2（只有一两个条目时占左半边），地与地之间是半格宽的街；平面无边，第 (列, 行) 块是 `(行 + 列 × stride) mod 块数`（`exploreLayout.ts`）。镜像位按组 key 的 FNV 哈希取（`mirrorsOf`），与原版 JS 对拍了同一组输入的座位输出，逐字相同。标题磁贴：角标「01 / 03」，标题是平台块名或 `home.dailyRecommendations` / `home.neteaseDailyPlaylists` / `home.neteaseCharts`，副标题如「12 · 专辑 · 歌曲」。缝的锚点在第 −1、0 列地之间那条街的中线。状态行：读取中 `playlist.loading`、失败 `home.exploreFailed`、空 `home.exploreEmpty`。
- 账户入口（`home/seam.rs` 的 `account_entry`，取数自 `BravaisSeamAccountSwitcher.tsx` 与 `bravaisAccount.css`）：只在在线页签，贴在工具格上方 8px（书脊 6px）。未登录：32px 圆（文字色 12% 底，Plug 16px），下面「连接在线平台」（12px，字重 800，可折行）。已登录：32px 圆头像（没有头像时是平台记号：红底白字「云」，14px 字重 900），下面昵称（单行）。内边距 8 4 6、圆角 12、悬停 12% 底；书脊上是 36px 按钮里 28px 的圆。悬停提示「连接在线平台 / 切换在线音乐平台 · 网易云 · 未登录 / 昵称」。点它打开登录窗口：已登录时开在网易云上，未登录时不替用的人选平台。
- 登录窗口（`login.rs`，取数自 `LoginHost.tsx`、`LoginScreen.tsx`、`LoginWindow.tsx`、`PlatformColumn.tsx`、`LoginTile.tsx`、`LoginMessage.tsx`、`loginView.ts`、`loginCaseTexts.ts`、`login.css`、`Sheet.tsx`）：遮罩深色黑 52%、日间 `rgba(24,18,12,0.36)`，点遮罩或 Esc 关闭。窗口 772 宽（窗宽不足时减去两边 16）、420 高，顶边在 `max(16, 窗高/2 − 210)`，圆角 16、`--st-win` 底、1px `line2` 描边、`0 36px 90px` 黑 60%（日间 26%）阴影；打开 300ms、关闭 220ms，曲线 `(0.22,1,0.36,1)`，透明度加 0.9→1 缩放。左边 212px 平台列（`--st-side`）：顺序同 `orderRows`（正在用的网易云在最前，其余按注册顺序）；行高把 420 平分后夹在 46–68；行里 30px 记号、13px 名字、11.5px 状态（已登录：绿点加昵称，右侧 24px 头像；登录过期：警告色「登录已过期」；未登录：「未登录」）；选中行左边一道 3px 平台色竖条，指针所在或选中的行填 `fill2`。另外 6 个平台照原版不可用的样子：透明度 0.42，不能点，状态和悬停提示是 `home.providerNotConfigured`（原版文案「Web 端未配置 API」）。右上是标题「登录到 网易云」（前面 24px 记号；没选平台时 `login.title`）和 32px 关闭（提示 `home.closeLogin`）。下面 200px 方块（圆角 12）加消息：没选平台是 88px 灰圆加 User；取码时是 30px 记号外一道转弧；二维码是白底 176px，出现时从左上角斜着揭开（480ms）；已扫码是 88px 圆加 Smartphone；读账号是 User 加转弧；失败是按类别着色的圆（A 灰 Clock、B 黄 WifiOff、C 红 Ban、F 红 UserX、G 红 CircleQuestionMark）；二维码过期时码淡到 10% 留在原地，上面 56px 深色圆钮刷新；已登录是 88px 头像（没有头像时是昵称首字），右下角 39px 平台记号。消息：标题 20px Semibold，第二行 13px `t2`，平台原话放在 `fill` 底的框里（平台名、原话、码），按钮 32px 高、13px，主操作是强调色；右下角步骤标记（二维码、手机、账号三个 26px 圆，已完成的填平台色，连线 26×1.5）。窗宽 <804 时平台列横到标题下面成一排，方块站到消息上面。等待扫码时从方块中心向外扩散三道平台色细圈（4.2s 一轮，错开 1.4s）。二维码由门面用 `qrcode` 按同一个地址在本地生成（原版是本地后端回的 `qrimg` 图片）。
- 扫码流程（同 `providerLoginSession.service.ts`）：选中网易云就取码；取到后马上查一次，之后每次结果回来再等 2 秒，不重叠；801 继续等，802 显示已扫码，已扫码超过 60 秒没确认按「未在手机上确认」结束；800 在扫码前是「二维码已过期」，扫码后是「未在手机上确认」；803 之后会话交给门面，在后台线程存进密钥库，再读账户；读到账户放登录成功动画，读不到是「读取账号失败」。轮询时没有状态码的网络错误容忍连续 2 次。失败情况照 `loginFailureCase.ts` 分：HTTP 429「要求稍后再试」，5xx 与没有状态码的网络错误「无法连接」，超时「无响应」，其余 4xx 与平台码「登录请求被拒绝」并引平台原话，其余「登录失败」；主操作都是「刷新二维码」（`home.retryQr`）。换平台、关窗都结束这一轮，晚到的回答作废。
- 登录成功动画（`loginDoneFrame.ts` 读到账号之后那一段）：平台色从方块中心铺满窗口（110–570ms，`inOut`），颜色是 135° 从亮一档到暗一档的渐变（`platformShades`，在 OKLCH 里只改明度）；右侧 196px 的平台字水印（10%）向左漂；88px 头像 200–640ms 从上方落下、从 1.2 缩到 1，右下角白底对勾（560–740ms）；右边「登录成功！」（13px）和昵称（28px Semibold）先后升起；1260ms 起窗口在 300ms 里放大到 1.08 并淡出，1800ms 结束。
- 退出登录：已登录时窗口里是账号那一幅（昵称、「网易云 · ID: …」、「退出登录」）。点「退出登录」先弹原版的确认框（`useProviderSignOut.ts` 的 `askProviderSignOut`：400px 宽，标题 `account.logoutConfirmTitle`，正文是昵称，播放器里有歌时加一行 `account.logoutClearsQueue`，按钮「取消」与红色「退出登录」）。确认后清空播放器（同 `resetForProviderSwitch`），通知平台 `logout`，删掉密钥库里的会话，墙回到访客说明。
- 在线页签、登录与原版的差异（只列 iced 做不到的）：

| 原版 | 这里 | 原因 |
|---|---|---|
| 页签、集合标题、竖排计数用 `writing-mode: vertical-rl`，拉丁字母侧转，长标题折成多列 | 逐字竖着摆，拉丁字母不侧转，长标题只排一列、超出裁掉；英文页签因此更高，更早缩成一个字 | iced 的文字没有竖排书写模式，也不能旋转字形；布局里没有按高度换列的竖排 |
| 登录窗口右上角平台色的 `radial-gradient` 底光（`LoginWindow.tsx:151`）；成功动画表面的两层 `radial-gradient` 高光与暗角（`platformShades.ts`） | 没有底光；成功动画只有 135° 线性渐变 | iced 0.14 的 `Background` 和 canvas 只有线性渐变 |
| 窗口 `overflow: hidden` 把左侧平台列和成功动画的铺色裁成圆角 | 平台列单独给左侧两个圆角，描边画在最上层；铺色用折线近似圆与圆角矩形的交集 | iced 的 `clip` 只按矩形裁剪，子元素不能裁成圆角 |
| 方块里的画每次换样子弹入（`login-pop`：0.94→1 缩放加淡入），标题和说明换样子时上移 4px 淡入（`login-in`） | 直接换 | iced 里缩放和平移子树要用 `float`，`float` 会把子树提到最上层，盖住确认框和标题栏 |
| 换页签、进出集合时整条缝像磁贴一样翻到新内容（`bravaisSeamMotion`：160ms `ease-in` 转到 90°，换内容，240ms `cubic-bezier(.2,.7,.25,1)` 转回） | 同样的时长与缓动，转出时还画旧内容（页签、集合信息），但转动画成内容透明度 = 转角的余弦（`home/flip.rs`） | iced 0.14 的 `Transformation` 只有平移和等比缩放，控件子树不能透视旋转，也不能单向压缩 |
| 磁贴翻牌（`perspective(1400px) rotateY`，`flipPlan` 18ms/格错开、上限 420ms，转出 150ms `cubic-bezier(.55,0,.9,.45)`，转进 210ms `cubic-bezier(.2,.7,.25,1)`） | 时长、错开、缓动照搬（`wall_shift.rs`），转动画成以磁贴中线为轴的水平压缩（宽 × cos 转角），没有透视的近大远小；文字不压缩，只裁在压窄的磁贴里 | canvas 没有透视变换；非等比缩放下 iced 把文字转成网格，网格画在所有图片之下 |
| 首页 ↔ 播放页：首页层 0.25s `easeInOut` 淡入淡出（`App.tsx`），下面的可视化一直在跑 | 离开的那一页截一帧（`window::screenshot`），盖在新页上 0.25s `easeInOut` 淡出（`page_fade.rs`）；淡出期间离开的那一页是静止的 | iced 没有整棵控件树的透明度；两个不透明层的交叉淡化与截图淡出结果相同 |
| 整墙波次、翻牌在降低动态效果时换成 0.18s 淡入淡出（`bravaisMotion`） | 没做 | 还没有「降低动态效果」设置（原版是设置里按界面勾选，默认关） |
| 一次滚动手势锁定在墙上 160ms（`WHEEL_LATCH_MS`），手势中滑到缝上也继续推墙 | 滚轮落在缝上就由缝处理 | iced 的 `Stack` 上层捕获事件后下层收不到，墙在缝下面 |
| 拖动速度用相邻两次 pointer 事件的系统时间戳算 | 用最近 48ms 内最早和最新的两次移动算 | iced 的事件没有时间戳，同一批处理的移动只差几微秒，相邻两次算出的速度会被顶到上限 4000px/s |
| 从搜索页 / 播放页打开集合的整墙入场（`enter`）与回去时的整墙出场（`exit`） | 没做 | 这里的搜索页只有歌曲，播放页的「定位」不经过这条路径，还没有入口会走到 |
| 字重 800（`font-extrabold`）、900、500 | 800、900 画成 700，500 画成 400 | cosmic-text 不合成字重，系统字体缺这一档时会换成别的字体；Windows 的 Segoe UI 没有 800 |
| 专辑海报的水印字是 1.5px 纸色描边（`-webkit-text-stroke`） | 纸色 7% 的实心字 | iced canvas 的文字只能填色，不能描边 |
| 海报的小标签旋转 −2° | 不旋转 | 小标签是文字控件，iced 的控件不能旋转（标题已改在 canvas 里画，放大的词照常错开旋转） |
| 海报的颗粒（`feTurbulence` 噪点 22%，`mix-blend-mode: overlay`） | 没有 | iced 没有混合模式 |
| 竖排碎字 `writing-mode: vertical-rl`、字距 0.3em | 逐字竖着摆 | 同上面竖排的那一条 |
| 海报这一列放不下时封面溢出、压在标题上 | 封面缩到这一格放得下 | canvas 按自身边界裁剪，控件不能画到布局边界外，`float` 会把封面提到最上层盖住按钮 |
| 聚焦卡的 `0 42px 110px` 投影、键盘焦点的 `0 12px 32px` 抬起、飞行中的软阴影 | 没有 | iced canvas 没有模糊阴影；硬阴影（无模糊）照画 |
| 飞行的封面按 `cover` 裁到转动中的矩形 | 角度为 0 时按矩形裁，转起来后画成长边大小的方图不裁 | canvas 的裁剪只有不转的矩形 |
| 碎片的 `grayscale/contrast/brightness` 滤镜、`multiply`/`lighten` 混合与网点 | 下载后在后台按同样的公式烘焙成一张图，网点烘焙成平铺的透明图 | iced 没有 CSS 滤镜和混合模式 |
| 拼贴磁贴文字的 `0 1px 2px` 阴影与 `0 0 14px` 光晕 | 下移 1px、60% 暗色的一份文字，没有模糊 | canvas 文字没有阴影 |
| 倾斜时海报的文字、封面与按钮跟着纸转，并缩放进安全区 | 海报内容保持竖直，两侧按 `窗高/2·tan(10°)` 往里收；纸、网纹、色块、水印在墙的 canvas 里跟着转 | iced 的控件不能旋转；常驻的缩放只能用 `float`，会把海报提到确认框和窗口之上 |
| 倾斜时磁贴按转后的形状裁剪封面 | 按转后矩形的外接框裁 | 同上，canvas 裁剪不转 |

- 在线部分还没做（原版有）：倾斜时拖动与滚轮沿纸的方向走（拖动和惯性的代码由另一处在改，这一轮不动）；聚焦卡「⋯」里的条目动作（移出歌单、不喜欢、手动匹配、加入歌单）与歌手 / 专辑链接（没有对应的页面）；「定位」后展开聚焦卡；悬停时磁贴外扩与列表面板 ↔ 墙的悬停联动；Tab 在缝与墙之间换站、在墙上打字即过滤；降低动态效果的设置；探索的后续部分与每栏的「更多」；FM 模式选择器与播放器面板的 FM 页签；登录窗口的自检、复制诊断与反馈、详情里的「时间」、详情展开动画、步骤标记脉冲、扫码阶段方块里的颜色生长、退出后的提示；「最近播放 / 再听一次」；播放列表的键盘操作与读屏播报。

- 主题：`folia_core::Theme::{MIDNIGHT, DAYLIGHT}` 为原版两套预设，`Shell` 由它生成 iced 调色板。
- 图标：`lucide-static-svg` 的 `Icon`，经 `icons::icon` / `icon_button` 着色绘制。
- 首屏遮罩（`splash.rs`，取数自 `index.html` `#app-splash` 与 `appSplash.ts`）：底色 `#09090b`，不跟主题。标题 `Folia` 五字，字号 `clamp(64px, 17vw, 200px)`、字重 600、行高 1、字距 0.06em、首行缩进 0.06em，与波形间距 28px。内容先透明，180ms 后用 CSS `ease`（`cubic-bezier(0.25, 0.1, 0.25, 1)`）在 420ms 内淡入。首页第一次收到资料库 `Loaded`（成功或失败都算），并且之后已经画出一帧时，整块遮罩立刻用同一条曲线淡出 320ms，然后从界面拿掉。这次就绪早于 180ms 时内容保持透明。波形 14 根，间距 `clamp(6px, 1.1vw, 11px)`，高 `clamp(42px, 7.5vw, 92px)`；每根 `scaleY` 在 `--floor` 与 `--peak` 之间按 `ease-in-out` 走 1100ms 的默认周期（实际 820–1500ms，相位为负延迟）。`--peak = min(1, 0.32 + sin((i+0.5)/n·π) · rand(0.38, 0.72))`，`--floor` 在 0.08–0.24，宽 3.5–6.5px，白色 alpha 0.55–1，辉光半径 6–26px。字母每 380ms 换 1 个，25% 的拍换 2 个；字族 BASE（`SansSerif`）、serif、monospace、cursive、fantasy、system-ui（`Segoe UI`），字重 400/600/800，25% 斜体，纵向缩放 0.86–1.12，横向再乘 0.86/0.94/1/1.06/1.12。盒子宽度是各字形在 100px、ExtraBold（含斜体）下的最大字宽 / 100 × 1.26 + 0.04em，只在内容还可能出现时测量；资料库在 180ms 内就绪则不测，也不建 canvas，遮罩只是一块随透明度变化的底色。canvas 画纯色盖层会留下约 20MB 的 GPU 专用内存。缩放原点在字盒垂直 78%。字母辉光不放大字形：按 `randomGlow` 三档取 1 或 2 个模糊半径（第三档省略 70–110px），每个半径在 1/3、2/3 和整半径处各 8 个方向，该层透明度均分到这 24 份，字号与正文字相同。只盖在整半径上时，8 个副本会分开成重影。竖条辉光是 6 层逐渐放大的圆角矩形，范围是 `--glow`，透明度按高斯衰减，各层之和为 0.35。未换字前的兜底辉光是样式表的 `8px / 0.55` 与 `28px / 0.3`。淡出一开始点击穿透，和 `.is-hiding { pointer-events: none }` 一样。计时从窗口第一帧起，不从进程启动起。
- 首屏与原版的差异：字距用固定盒子加 0.06em 间隙，不是 CSS `letter-spacing`。辉光是同字号的环形偏移和分层圆角矩形，不是 `text-shadow` / `box-shadow` 的真实高斯模糊；第三档最外圈 70–110px 不画。字母只有 1:1 的那一档进字形图集：iced 会把等比缩放后的字号各留一份且不缩小，比例相等又不等于 1 时改为略微不等的轮廓缩放。没有单独的 Inter 字体，BASE 用 iced 的 `SansSerif`，`system-ui` 用 Segoe UI。不读 `prefers-reduced-motion`。遮罩不覆盖进程启动到窗口出现之前的时间。
- 重绘：播放定时器只在播放中订阅。正在播放页按帧率上限（`FrameRate::Display` 用 `window::frames()`），其他页 250 ms。浮层的 200 ms 检查只在隐藏期限还在时订阅；胶囊的 16 ms 只在悬停延迟或宽度动画还没结束时订阅。首屏的逐帧订阅只到淡出结束。首页缝、面板、返回和菜单只在补间进行中逐帧订阅。登录窗口只在开合补间、取码与读账号的转弧、二维码揭开与等待扫码的扩散圈、已扫码和登录成功动画期间逐帧订阅；扫码轮询不用定时器，每次结果回来后起一个线程睡 2 秒再发下一次。播放器面板只在开合或把手进出的补间里逐帧订阅。搜索浮层只在进出场的 200ms 里逐帧订阅。不播放、浮层已隐藏、遮罩已摘掉、补间已结束时没有定时器。
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

- 首页为原版 Bravais 封面墙，可拖动、滚轮平移、点击播放；能扫描本地文件夹、播放、暂停、跳转、调音量、切换重复模式；搜索并播放网易云歌曲（含歌词）；首页有歌单 / 电台 / 探索 / 专辑 / 本地五个页签，网易云扫码登录后在线页签铺用户歌单、电台、探索与收藏专辑，点进集合可看曲目并播放（扫码登录和账户接口尚未在真机上验证）；显示流光/云阶/静止/倾诉/回环/心象歌词样式与翻译；切换语言、主题、歌词样式、帧率上限。
- release 构建（Windows，Intel Arc A580）：空闲 CPU 0%；正在播放页 60 FPS 动画时 CPU 0.16%（20 逻辑核折算）、GPU 3D 约 3.5%、工作集约 116 MB。首页封面墙（26 首）静止时 CPU 0%、GPU 0%，持续拖动时 CPU 0.06%、GPU 3D 3.7%，工作集约 130 MB、私有内存约 230 MB。
