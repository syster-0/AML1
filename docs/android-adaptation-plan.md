# AML 安卓适配方案（Rust 核心路线）

> 创建日期：2026-09-24
> 状态：Phase 0–7 全部验收通过（JRE21/Fabric/Forge/NeoForge 均端到端验证，2026-09-27，见 7.1–7.3）；Phase 8+ 待评估
> 决策前提：以 **Rust 为核心**做安卓适配，Kotlin 仅作薄壳承载，所有业务逻辑（启动、JVM 创建、桥接）放在 Rust。

---

## 一、核心结论

1. 在安卓上运行 Minecraft Java Edition 的关键技术栈是：

   > **JRE 运行时 ＋ 两套游戏运行时（PojavLauncher 路线 / Boat 路线）＋ 渲染器 ＋ 输入系统**

2. AML 现有基础（Rust cdylib + cargokit + FRB + 与平台无关的参数构建）天然适配这套技术栈。
3. AML 的差异化路线：**启动编排、JVM 创建、原生桥接全部用 Rust 自研实现**，Kotlin 只剩 Activity 承载骨架。
4. 所有代码注释、文档、提交信息按第六节的规范书写，注意合规与措辞。

---

## 二、安卓运行 MCJE 的关键技术组成

### 2.1 JRE 运行时

- 安卓不能跑桌面 glibc 版 JRE，需要 **Bionic 版 OpenJDK**（基于 Termux 生态构建）。
- 覆盖 Java 8 / 17 / 21（Minecraft 实际使用，按版本自动选择），另提供 25（LTS，面向未来，视运行时构建产物而定）；arm64 为主，x86_64 供模拟器使用。
- JRE 以应用内下载方式分发，解压到应用私有数据目录。
- 构建来源：`android-openjdk-build-multiarch`（OpenJDK 本身为 GPL + Classpath Exception）。

**Java 设置页面（安卓适配要点）：**

现有 `lib/src/features/settings/ui/java_settings_page.dart` 与 `widgets/java_selector.dart` 是桌面形态，安卓上需改造：

- **版本列表**：与桌面保持一致，25 / 21 / 17 / 8。其中 21/17/8 为 Minecraft 实际使用版本；25 是面向未来的 LTS，当前无 MC 版本要求，**是否可下载取决于安卓运行时构建是否提供产物**，不预先砍掉。
- **路径输入框取消**：安卓受分区存储限制，不能让用户手填或浏览文件系统里的可执行路径；路径只读，指向应用私有目录内的运行时位置。
- **检测按钮取消**：桌面版扫描系统已装 JRE 的逻辑在安卓不适用。
- **浏览按钮改为导入运行时**：从文件选择器导入运行时压缩包（解压到私有目录），而非选择可执行文件。
- **安装推荐版本保留并作为主操作**：展示每个大版本运行时的状态（未安装 / 下载中（带进度）/ 就绪 / 校验失败），点击下载。
- **测试按钮改为运行时校验**：校验已安装运行时的完整性与版本，而不是探测外部路径。
- **设置持久化**：存储选中的运行时标识与版本，而非可执行文件路径。
- 页面交互组件避免依赖 hover / MouseRegion，改为触控友好形态。

### 2.2 两套游戏运行时

Minecraft 不同版本使用不同代际的 LWJGL，需要两套运行时分别覆盖：

| 运行时路线 | LWJGL 代际 | 覆盖版本 | 实现要点 |
|---|---|---|---|
| **PojavLauncher 路线** | LWJGL 3.x | **MC 1.13+**（主力） | 重写 GLFW backend（GLFW stub），通过 EGL 桥接 Surface；1.12.2 及以下在该路线上通过 lwjglx 兼容层支持 |
| **Boat 路线**（BoatApp 及衍生项目） | LWJGL 2.x | **MC 1.0–1.12** 老版本 | LWJGL2 安卓原生移植，独立的 input/egl 通道 |

结论：**新版本走 PojavLauncher 路线，老版本走 Boat 路线**，两套都需要，只是优先级不同（先做 PojavLauncher 路线）。

### 2.3 渲染器

Minecraft 使用桌面 OpenGL，安卓只有 OpenGL ES / Vulkan，必须通过翻译层：

| 渲染器 | 原理 | 许可证 | 适用 |
|---|---|---|---|
| **GL4ES** | 桌面 GL → GLES 2.0 | MIT | **首发唯一承诺的后端**，覆盖老设备/老版本 |
| **GL4ES+（GLES3 模式）** | 桌面 GL → GLES 3.0 | MIT | 现代设备性能更好 |
| **Zink** | OpenGL over Vulkan，需安卓 Vulkan + 为安卓交叉编译 Mesa | MIT（Mesa） | 后期，工程量大 |
| **VirGL** | 安卓语境下的特定移植/包装（非 QEMU host 侧 virglrenderer） | MIT（Mesa） | 后期，需按安卓语义理解 |
| **ANGLE** | GL → 各厂商原生图形 API | BSD-3 | 可选备选 |

首发只实做 GL4ES；`RenderBackend` trait 保留插槽，但 zink/virgl 不预埋空壳。

### 2.4 音频

- **OpenAL Soft**（开源，LGPL/BSD 双许可）：LWJGL 的 OpenAL API 在安卓上的实现后端。

### 2.5 输入系统（暂缓，非首发目标）

> **优先级说明：输入系统不着急，首发目标是把游戏运行起来。** 首发只需最小可用的触控转视角，下列完整能力放到后期。

- 触控手势 → 视角拖拽（首发最小实现）
- 虚拟鼠标光标（菜单与第一人称两种捕获模式）
- 按键布局映射：可移动/缩放的虚拟按键，支持外接键鼠与手柄
- 输入管道尽量走 native，避免频繁跨语言调用

### 2.6 无 X11 的 AWT 支持

- 模组安装器等 Swing 程序需要在无 X11 环境下渲染与接收输入（Caciocavallo 同类机制：AWT 图形调用转发到原生画布）。
- 后期需要，首发可只做 headless 安装。

### 2.7 数据保存目录（安卓差异）

桌面由 `getApplicationSupportDirectory()` 解析（Linux：`~/.local/share/com.astral.aml`）；安卓完全不同：

| 目录类型 | 实际路径 | 特点 |
|---|---|---|
| **应用私有内部目录**（默认） | `/data/user/0/com.astral.aml/files`（即 `context.filesDir`） | 无需任何权限，其他应用不可见，卸载即删除；`path_provider` 的 `getApplicationSupportDirectory()` 在安卓即解析到此 |
| **应用专属外部目录**（可选） | `/storage/emulated/0/Android/data/com.astral.aml/files`（即 `getExternalFilesDir()`） | Android 4.4+ 无需存储权限，文件管理器可见，卸载即删除 |
| 公共存储 / 自定义路径 | —— | **Android 11+ 分区存储（Scoped Storage）下不可自由使用**，不支持 |

要点：

- **根目录由系统决定，不能让用户自由填写或浏览任意路径**；只能在「内部目录 / 外部专属目录」之间选择。
- 根目录下的内部布局（`meta/`、`instances/`、DB、运行时等）保持现有 `dirs.rs` 拓扑，**无需改动**，只是根路径前缀不同。
- JRE 运行时、natives、缓存同样放在应用专属目录下，随应用卸载清除。
- 这些目录中的普通文件**没有执行权限**，所以需要执行的库一律以 jniLibs 打包或 dlopen 方式处理（见第三节）。
- 切换内部/外部目录属于数据迁移场景，需实现数据搬移与校验，不能静默丢数据。

---

## 三、进程、任务与 JVM 模型

### 3.1 为什么是进程内 JVM（指游戏进程内）

游戏不用 `java -jar ...` 拉外部可执行进程，而是在**游戏进程内**通过 **JNI Invocation API（`JNI_CreateJavaVM`）创建 JVM**，由系统平台限制决定：

1. **W^X 执行限制**：Android 10+ 禁止从应用数据目录执行文件。但以 `lib*.so` 形式打包进 jniLibs 的库系统解压时自带执行位，且**允许 dlopen 应用私有目录中的 .so**。
2. **Linker namespace**：Android 7+ 限制随意 dlopen 系统私有库；安卓版 OpenJDK 自带全部依赖，自有桥接代码走正常 JNI 加载，不碰系统私有库。
3. JVM 在进程内创建后，Surface、输入事件、日志可在该进程内直接传递，无需跨进程序列化。

### 3.2 双进程：启动器进程 ＋ 游戏进程

游戏 Activity / Service 声明在**独立进程**（`android:process=":game"`），与启动器分开：

```
┌─ 默认进程 com.astral.aml ────────┐   ┌─ 游戏进程 com.astral.aml:game ──────┐
│  Flutter UI (Dart)               │   │  GameActivity (SurfaceView)       │
│  Rust: 下载/安装/账号/参数构建   │   │  Rust: ffi/runtime/loader/surface  │
│         │                        │   │     dlopen libjvm → CreateJavaVM   │
│         │ 写入启动清单 JSON      │   │     Minecraft + LWJGL 运行时        │
│         ▼                        │   │  GL4ES → ANativeWindow              │
│  Intent 携带清单路径拉起 ────────┼──▶│                                    │
└──────────────────────────────────┘   └─────────────────────────────────────┘
```

独立游戏进程的好处：

- JVM / 渲染崩溃不拖垮启动器；可独立杀游戏
- 游戏内存与启动器隔离
- 启动器通过启动清单文件（实例 id、classpath、JVM 参数、渲染器）与游戏进程解耦，**不依赖共享内存状态**

### 3.3 最近任务显示两张卡片（可以做到）

「最近任务」按 **Task**（而非进程）组织。游戏 Activity 使用独立 `taskAffinity` + `launchMode="singleTask"`，即可在最近任务中看到 **启动器与游戏两个独立卡片**，各自独立切换。`FLAG_ACTIVITY_NEW_DOCUMENT` / `documentLaunchMode` 留到多开再评估（部分国产 ROM 对 NEW_DOCUMENT 支持不稳定，首发尽量少依赖）。

### 3.4 同时启动多个 MC：可行但有硬约束，作为后期目标

约束：

- **一个进程只能创建一个 JVM**：HotSpot 不支持同进程内多个 `JNI_CreateJavaVM`。因此「多任务卡片但同进程」无法支撑多开。
- 多开必须**每个实例一个独立游戏进程**。安卓的 `android:process` 在清单中静态绑定组件，运行时不能为同一 Activity 动态分配新进程。
- **不要用 `activity-alias` 做多开槽位**：`<activity-alias>` 只允许 enabled/exported/icon/label/name/permission/targetActivity 等属性，**不能**声明 `android:process` / `taskAffinity` / `launchMode`，这些全部继承 target。正确做法是预置多个真实 Activity：`GameSlot1Activity`…`GameSlot4Activity`，各自 `android:process=":gameN"`，共用 `BaseGameActivity`。
- 首发只建一个 `GameActivity`（`:game`），**不在 Phase 0–5 预埋多开槽位**，避免干扰"一个 :game 进程里 JVM 能否起来"的调试。
- 内存门槛按版本区分：**原版（尤其老版本）1GB 级 `-Xmx` 即可运行**；两个轻量 / 原版实例在 6–8GB 设备上有可行性。高版本 + 模组 / 整合包每个实例 2–4GB，同时两个才需要 12GB 以上设备。
- 默认策略：多开入口按设备总内存 + 实例类型（原版 / 模组）动态开放，而非一刀切。

结论：

> **首发：双进程、两张最近任务卡片、单游戏实例。架构上按 3.2 的启动清单解耦，不封死多开；多开（进程池 + 槽位调度）作为后期独立阶段。**

### 3.5 多开就绪架构对单实例零损耗

为多开预置的能力在只运行一个实例时不产生运行时成本：

- 多开槽位 Activity 是静态清单组件，不启动即零占用；
- 槽位调度是一次微小查表；
- 启动清单解耦仅在启动瞬间多一次几 KB 文件读写；
- 独立游戏进程是单实例也需要的基础设计，非多开额外代价。

性能与内存只随**实际启动的 JVM 数量**增长；架构本身不向单实例征税，唯一成本是工程复杂度。

---

## 四、AML 现状与差距

### 4.1 已有基础

- `android/` 脚手架完整，`MainActivity.kt` 存在。
- `rust_builder/android` ＋ cargokit 已具备把 Rust 编成 Android `.so` 的构建通路（`rust_builder/cargokit/build_tool/lib/src/android_environment.dart` 已处理 NDK 环境），FRB 官方支持 Android。
- `rust/src/launcher/args.rs` 的版本 JSON 解析、规则判定、占位符替换（`${natives_directory}` 等）**与平台无关，可直接复用**。

### 4.2 五处桌面假设必须改造

1. **进程模型**：`rust/src/launcher/process.rs` 当前是 `Command::new(java)` 拉外部进程＋stdout/stderr 管道＋TCP RPC。安卓上改为**同进程 JNI 启动**，日志走 native 回调，Theseus RPC 以 JNI 直接调用或 loopback 替代。
2. **JRE 来源**：`rust/src/api/java_download.rs` 当前走 Azul API 的桌面 JRE，安卓改为 Bionic 版 OpenJDK 分发，Java 检测增加 android target。
3. **LWJGL 替换**：`args.rs` 产出通用 LaunchConfig 后，由 **Android Adapter**（`vendor/lwjgl.rs`）改写 classpath / natives / 系统属性，注入渲染器选择与相关的 `org.lwjgl.*` 属性；不把安卓特殊逻辑塞进通用版本规则。
4. **数据保存目录**：根目录由系统决定（应用私有内部目录 / 外部专属目录），不支持自定义路径；内部布局复用现有 `dirs.rs`，仅前缀不同，切换目录需做数据迁移（见 2.7）。
5. **UI/交互**：现有 Flutter UI 基于窗口、hover、托盘，触屏交互另行适配。

---

## 五、Rust 核心方案

### 5.1 总体架构

双进程结构详见 3.2。各进程内 Rust 模块的职责分布：

```
默认进程（启动器）                         游戏进程（:game）
┌─────────────────────────────┐           ┌─────────────────────────────┐
│ Flutter UI                  │           │ GameActivity → Surface      │
│ Rust（现有模块，直接复用）：  │  启动清单  │ GameService（前台服务）      │
│  args.rs / manifest / rules │  JSON +   │ Rust（新增 android 模块）：  │
│  install / download / auth  │  Intent   │  runtime/jvm.rs JVM 创建    │
│  DB / jre 下载管理          │ ────────▶ │  ffi/bridge.rs JNI 入口     │
│                             │           │  ffi/surface.rs 原生窗口   │
│                             │           │  input/touch.rs 最小触控   │
└─────────────────────────────┘           │      │ dlopen               │
                                          │      ▼                      │
                                          │  libjli → JNI_CreateJavaVM  │
                                          │  MC + LWJGL 运行时           │
                                          │  GL4ES/Zink → ANativeWindow │
                                          └─────────────────────────────┘
```

### 5.2 模块拓扑与分层

安卓专属代码收敛在主 crate 的 `rust/src/android/`（同一 cdylib 会被主进程与 `:game` 进程各自加载，`JNI_OnLoad` 按 `/proc/self/cmdline` 分流）。落地后的实际结构如下（相对原设计的偏差：`ffi/` 平铺为 `bridge.rs`/`surface.rs`；`vendor/`、`render/`、`input/` 子目录尚未建，JRE 供应暂由 `runtime/jre.rs` 承担，GL4ES 桥以独立 crate 承载——见 5.4(7)）：

```
rust/src/android/
├── mod.rs                  # JNI_OnLoad + 按 /proc/self/cmdline 分流（main / :game）
├── bridge.rs               # Java_* JNI 入口（registerNatives），捕获 ART JavaVM*
├── surface.rs              # Surface ↔ ANativeWindow，attach 时注入 EGL 桥
├── launch.rs               # LaunchManifest 构建：classpath 改写、LWJGL 属性、env
├── pojav_bridge.rs         # dlopen libpojavexec.so 的动态绑定（双 VM 显式注册）
└── runtime/
    ├── mod.rs              # 游戏运行时编排
    ├── loader.rs           # 绝对路径 dlopen libjsig → libjli → libjvm（RTLD_GLOBAL）
    ├── jvm.rs              # JNI_CreateJavaVM + 应用 env + 调 Minecraft main
    ├── jre.rs              # JRE 树定位/校验
    └── log.rs              # 游戏 VM 日志 → UDS → 启动器

rust/pojavexec/             # 独立 crate：build.rs 直接调 NDK clang 链接 C 源，
├── c/                      #   产出 libpojavexec.so（GLFW stub 硬编码的库名）
└── src/lib.rs              #   Rust 侧为空壳，符号全部由 C 导出
```

**分层与依赖方向（只允许向下依赖）：**

```
bridge/surface → runtime                 入口只做转换与兜底，不含业务
runtime        → launch + pojav_bridge   编排：先供应，再注入桥，后启动 JVM
pojavexec      → （独立 C 库）            Rust 只 dlopen，不静态链接
```

**设计原则：**

1. **渲染器是插件，不是分支**：定义 `RenderBackend`（标识、所需环境变量、初始化窗口、反初始化），各后端实现同一 trait，运行时按注册表与设备策略选择；新增后端不改编排代码。

   ```rust
   pub trait RenderBackend {
       fn id(&self) -> &str;
       fn env_vars(&self) -> Vec<(String, String)>;
       fn attach_window(&self, window: *mut c_void) -> Result<()>;
       fn shutdown(&self);
   }
   ```

   （注：GL4ES 路径实际未走该 trait——GL 上下文由 `libpojavexec.so` 的 EGL 桥在 HotSpot 侧建立，`RenderBackend` 留待 Zink 等需要 Rust 侧参与的后端再启用，不预埋空壳。）

2. **运行时区分「供应」与「执行」**：JRE/LWJGL 准备好并校验（`runtime/jre.rs`、launch 清单组装），`runtime/` 负责同进程启动与生命周期，两者不混在一个文件里。
3. **FFI 边界收敛**：`#[no_mangle]` 与 panic 兜底只出现在 `bridge.rs`/`surface.rs`；内部模块全部返回 `Result`，不感知 JNI。
4. **复用而非分叉**：现有 `args.rs` 产出的启动配置作为 `launch.rs` 清单的输入，安卓侧只做平台改写（GLFW stub jar 前置、GL4ES 属性、env 注入），不重写版本/规则逻辑。
5. **平台隔离**：`rust/src/android/` 与 `rust/pojavexec/` 仅面向 Android target，桌面构建不编译这些模块（`#[cfg(target_os = "android")]`）。

### 5.3 技术选型（具体 crate）

| 用途 | crate | 说明 |
|---|---|---|
| JNI 绑定 | `jni = "0.21+"` | 需按实际版本核对 API；`EnvUnowned` 等命名可能已变 |
| 加载 JRE | `libloading` | 按 `RuntimeLayout` 绝对路径 dlopen `libjvm.so`，`dlsym("JNI_CreateJavaVM")` |
| 原生窗口 | `ndk = "0.9"` | 用 `ANativeWindow_fromSurface` + `acquire`（API 以当时文档为准） |
| 日志 | `android_logger` + `log` | Rust 侧直接写 logcat |
| 异步 | `tokio`（已在用） | Android 上完整可用 |
| 构建 | 现有 cargokit | NDK 环境已支持 |
| Dart↔Rust | 现有 FRB | 启动器侧调用方式不变 |

**不使用 `android-activity`/`android_main` 模式**——那是给纯 Rust UI 的 NativeActivity/GameActivity 应用用的。AML 的 UI 是 Flutter，Rust 只做被调用方。

### 5.4 关键模块实现要点

#### (1) `runtime/loader.rs` — 运行时加载器（一等模块）

Android 7+ 的 linker namespace 下，普通 `dlopen("libjvm.so")` 会因搜索路径不含 JRE 目录失败。加载器职责：

1. 读 `RuntimeLayout`（由 `vendor/jre.rs` 解压后写出，禁止写死 arch/子目录）
2. 校验 ELF 架构 + 16KB 页对齐
3. 按依赖顺序用**绝对路径** `dlopen`（`RTLD_NOW | RTLD_GLOBAL`）：`libjsig.so` → `libjli.so` → `libjvm.so` → 其余依赖
4. 失败时把 `dlerror` 和已加载列表写 logcat
5. `dlsym(libjvm, "JNI_CreateJavaVM")`
6. 所有 library handle 进程内永不 `dlclose`

```rust
pub struct RuntimeLayout {
    pub java_major: u32,
    pub arch: String,
    pub jvm_library: PathBuf,         // libjvm.so 绝对路径
    pub jli_library: Option<PathBuf>,
    pub jsig_library: Option<PathBuf>,
    pub native_dirs: Vec<PathBuf>,
    pub boot_library_path: PathBuf,
}
```

**关键纠正**：`JNI_CreateJavaVM` 的实现在 **`libjvm.so`**（通常 `lib/server/libjvm.so`，Java 8 为 `lib/<arch>/server/libjvm.so`），**不在 `libjli.so`**。`libjli.so` 是 launcher 基础设施，只是它内部再去 dlopen libjvm。所以必须从 libjvm 取符号。

**信号链**：加载 `libjvm.so` **之前**先加载同套 JRE 的 `libjsig.so`。ART 用 SIGSEGV 做隐式空指针检查，HotSpot 也需要 SIGSEGV/SIGPIPE/SIGBUS 等；两个 VM 同进程必须靠 signal chaining 避免冲突，不要自己再装 SIGSEGV handler。

#### (2) `runtime/jvm.rs` — 创建 JVM 并执行 main（必须在游戏线程）

```rust
/// 在专门游戏线程调用；本调用会把当前线程变成 HotSpot main thread。
pub fn start_jvm(layout: &RuntimeLayout, class_path: &str, jvm_args: &[String]) -> Result<()> {
    let create_jvm = loader::load(layout)?;  // loader.rs 完成 dlopen+dlsym
    // 注意：JVM option 字符串用 CString 持有到 JNI_CreateJavaVM 返回，不能传临时 format!() 的引用
    // 这里省略具体 InitArgs 构造，jni crate 版本不同 API 有差异，Phase 0 先做最小探测
    // ...
    // rc == JNI_OK 后调用 Minecraft main，阻塞直到退出
    Ok(())
}
```

要点：

- **必须在专门游戏线程调用** `JNI_CreateJavaVM`，该线程会成为 HotSpot main。若在 `surfaceCreated`（UI 线程）调用，UI 线程变成 MC 主线程直接 ANR。
- 启动 VM 后用**游戏 VM** 里的 `System.out/System.err` 替换为写 UDS 的 PrintStream，或给 log4j 配 socket/file appender——**不要 `dup2(1/2)`**，那会把整个进程（含 ART/logcat）输出切走。

#### (3) `ffi/bridge.rs` — ART 边界（只转换，不承载业务）

游戏进程里同时存在两套 VM：ART（Kotlin 跑）和 HotSpot（MC 跑）。**禁止**把 ART 的 `JNIEnv*` 传进 MC/LWJGL；`Java_*` 里拿到的 env 只允许：读 jstring、从 Surface 取窗口、把事件丢进队列、起/停 runtime 线程。

```rust
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_astral_aml_game_GameBridge_nativeStart(
    env: jni_sys::JNIEnv, _: jni_sys::jclass, manifest_path: jni_sys::jstring,
) {
    catch_unwind(|| {
        let path = read_jstring(env, manifest_path); // 校验在私有目录内
        spawn_runtime_thread(path);                  // 立即返回，不阻塞 ART
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_astral_aml_game_GameBridge_nativeSurfaceCreated(
    env: jni_sys::JNIEnv, _: jni_sys::jclass, surface: jni_sys::jobject,
) { catch_unwind(|| surface::attach(env, surface)); }  // ANativeWindow_fromSurface + acquire，投递 AttachWindow

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_astral_aml_game_GameBridge_nativeSurfaceDestroyed(
    _: jni_sys::JNIEnv, _: jni_sys::jclass,
) { catch_unwind(|| surface::detach()); }  // 投递 DetachWindow，JVM 继续存活

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_astral_aml_game_GameBridge_nativeSendInput(
    env: jni_sys::JNIEnv, _: jni_sys::jclass, event_json: jni_sys::jstring,
) { /* 投递输入事件到队列 */ }

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_astral_aml_game_GameBridge_nativeStop(
    _: jni_sys::JNIEnv, _: jni_sys::jclass,
) { /* 投递停止，join runtime 线程 */ }
```

#### (4) `ffi/surface.rs` — Surface → ANativeWindow

```rust
// 用 ndk 的 ANativeWindow_fromSurface + acquire；API 以当时版本文档为准
let window = unsafe { ANativeWindow_fromSurface(env, surface) };  // 概念示意
// acquire 后跨线程投递；Surface 对象不要长期握 JNI 全局引用当渲染目标
```

Surface 生命周期与 JVM 生命周期分离：`surfaceDestroyed` 只 detach 窗口，`JVM` 继续存在，以支持锁屏/切后台/Activity 重建。

#### (5) `vendor/jre.rs` — JRE 分发与校验

替代桌面 `rust/src/api/java_download.rs`：维护安卓 JRE 版本清单（Java 8/17/21 必需，25 视构建产物提供，arm64/x86_64），下载、校验、解压到应用私有目录，并**写出对应 `runtime.json`（RuntimeLayout）**。解压后做 16KB 对齐检查，不合格标「校验失败」，不要等到 dlopen 才爆。

#### (6) 进程模型分叉

`rust/src/launcher/process.rs` 的 `spawn` 按平台分叉：

```rust
#[cfg(target_os = "android")]
{
    // 不走 Command：写 launch_manifest.json，返回 handle（instance_id + 清单路径）
    // 真正 startActivity 由 Dart/Android 插件执行；游戏进程没有 MethodChannel
}
#[cfg(not(target_os = "android"))]
{
    // 现有外部进程/命名管道逻辑原样保留
}
```

`JNI_OnLoad` 按 `/proc/self/cmdline` 分流：默认进程 → 初始化 FRB/启动器侧；`:game` 进程 → 只注册 GameBridge native 方法，不启动 FRB/Flutter。同一 cdylib 会进两个进程。

#### (7) `libpojavexec.so` — Phase 5 的 EGL/输入桥（关键事实修正）

**事实修正（Phase 5 调研结论，推翻原设想）：**

1. PojavLauncher v3 路线**不存在「原生 libglfw.so + ANativeWindow 后端」**这种组件。真实结构是：Java 假 GLFW stub（`org.lwjgl.glfw` 全部假实现，window 指针伪造）+ `libpojavexec.so` 内的 EGL 桥（`pojavInit` → `eglCreateWindowSurface(ANativeWindow)`，GL 函数指针由 GL4ES 提供）。
2. **归档的 `lwjgl3-glfw-java` stub 是 LWJGL 3.2.x 世代**，依赖的 `DynCallback`/`CallbackI.V` 在 LWJGL 3.3.0 已删除，对 MC 1.13+ 所需的 3.3.x **不可用**。3.3.1 的安卓 stub 是预编译 jar：`lwjgl-glfw-classes.jar`（PojavLauncher `v3_openjdk` 的 assets 内）。
3. 3.3.1 的 native 契约**不是** 3.2.x 的 `nativeEgl*`，而是一组 C 符号 `pojavInit/pojavCreateContext/pojavMakeCurrent/pojavSwapBuffers/pojavSwapInterval/pojavSetWindowHint/pojavGetCurrentContext/pojavTerminate/...`（stub `GLFW` 经 dlsym 解析）+ 少量 JNI 方法（`nglfwSet*Callback`、`CallbackBridge.*`），且 stub 静态初始化硬编码 `System.loadLibrary("pojavexec")`。

**决策：AML 自建 `libpojavexec.so`（`rust/pojavexec/` 独立 crate）**，只导出上述契约符号，不引入上游第二套 JVM 引导与 stdio 劫持。理由（与许可证无关，内部验证路径下直接用 GPL 组件合规）：

- 上游 `libpojavexec.so` 是其整个原生层的打包（JVM 引导 + EGL 桥 + 输入桥 + OSMesa/Zink/Turnip 分支），整包引入会与 AML 已验收的 Rust `runtime/jvm.rs` 引导冲突，并带入大量走不到的死代码；
- 但 EGL 桥核心（`EGL_BAD_SURFACE` 恢复、surface 销毁切 1×1 pbuffer、resize 状态机、GL4ES 尺寸回调）是上游多年踩坑成果，按内部验证路径**直接保留其 C 实现并裁剪**，不从零重写。

**实现要点：**

- **构建**：`build.rs` 直接调 NDK clang 链接 C 源产出 `libpojavexec.so`，不走 rustc cdylib 空壳（rustc 版本脚本的 `local: *` 会压过导出参数，whole-archive 拉入的 C 对象会被 `--gc-sections` 回收）。产物手动放入 jniLibs（x86_64 + arm64-v8a），与 JRE 部署同模式，不动 Gradle/cargokit。
- **双 VM 注册**：上游依赖同进程两次 `JNI_OnLoad`（linker namespace 副作用）把桥同时挂到 ART 与 HotSpot，在 AML 的确定性引导里不可靠。改为导出 `pojav_bridge_on_art(vm, env)` / `pojav_bridge_on_runtime(vm, env)` 两个显式初始化入口：ART 侧在 surface attach 时调用，HotSpot 侧在 `JNI_CreateJavaVM` 成功后、游戏代码触碰 LWJGL 前调用（[jvm.rs](file:///home/kevin/code/AML/rust/src/android/runtime/jvm.rs)）。
- **窗口注入**：`surface.rs` 在 attach 时把 `ANativeWindow*` 经 `pojav_set_bridge_window` 写入桥的全局状态，`pojavNotifyWindow` 通知下次 swap 重建 EGL surface；detach 传 NULL。EGL 上下文在 HotSpot 侧建立，Rust 不持有。
- **Rust 侧动态绑定**：[pojav_bridge.rs](file:///home/kevin/code/AML/rust/src/android/pojav_bridge.rs) 用 `dlopen`/`dlsym` 一次性解析 4 个入口，主 crate 不静态链接该库。
- **classpath/env 注入**（[launch.rs](file:///home/kevin/code/AML/rust/src/android/launch.rs)）：stub jar 前置于 classpath 顶部遮蔽桌面 `lwjgl-glfw-3.3.1.jar`；注入 `-Dorg.lwjgl.opengl.libname=libgl4es_114.so`；清单新增 `env` 字段携带 `POJAV_RENDERER=opengles`、`LIBGL_ES=2`、`POJAV_NATIVEDIR=<natives>`，由 `jvm.rs` 在建 VM 前 `set_var`。
- **ART 侧 CallbackBridge**：`android/app/src/main/java/org/lwjgl/glfw/CallbackBridge.java`（包名须与 stub 期望一致）承接 Android→桥的回调；触摸接线属 Phase 6。
- **natives**：GL4ES（`libgl4es_114.so`）、OpenAL Soft（`libopenal.so`）、LWJGL natives（`liblwjgl*.so`、`libfreetype.so`）取 x86_64/arm64 预编译产物，放游戏 natives 目录。若 LWJGL 3.3.1 jar 与预编译 natives 出现 `org.lwjgl.system` 版本签名不匹配，需用 NDK 从 LWJGL 3.3.1 源码重建 natives。

### 5.5 启动时序

```
Dart: launchInstance()
  → FRB → Rust（args.rs 组装 classpath 参数，与桌面同一套代码）
      → Android Adapter: 解析 javaVersion → RuntimeLayout
          → JRE 就绪检查（缺失就拒绝启动，不在游戏进程现下）
          → 改写 classpath/natives/系统属性
          → 按设备内存写入最终 -Xmx
          → 写 app-private launch_manifest.json
      → 返回 LaunchHandle { instance_id, manifest_path }
  → Dart/Android 插件 startActivity(GameActivity, extra=manifest_path)

GameActivity.onCreate (:game 进程, ART)                # 非 FlutterActivity
  → 校验清单路径落在私有目录                  # exported=false，防任意代码执行
  → GameBridge.nativeStart(path)               # ART JNI，立即返回
        Rust: 读清单，spawn runtime 线程

GameSurfaceView.surfaceCreated
  → nativeSurfaceCreated(surface)               # ANativeWindow_fromSurface + acquire，投递 AttachWindow

runtime 线程（非 UI、非 FRB）
  → 应用清单 env（POJAV_RENDERER / LIBGL_ES / POJAV_NATIVEDIR）
  → loader: libjsig → libjli → libjvm → 依赖库
  → JNI_CreateJavaVM(HotSpot)                   # 此线程成为 MC main
  → pojav_bridge::init_runtime(vm, env)          # EGL/输入桥注册 HotSpot VM（Phase 5）
  → 设置游戏 VM System.out/err → UDS
  → CallStaticVoidMethod(Minecraft main)        # 阻塞直到退出；GL 上下文由桥在 HotSpot 侧建

surface attach（可先于/后于 JVM 创建）
  → pojav_bridge::init_art + set_window(ANativeWindow)   # 桥持窗口，glfwInit 时建 EGL surface

surfaceDestroyed
  → nativeSurfaceDestroyed → DetachWindow       # JVM 仍在

退出
  → 游戏 main 返回或 abort
  → UDS 发 finished → 启动器 ProcessManager 发已有事件 → GameActivity.finish()
```

### 5.6 Kotlin 薄壳范围（首发三 + 一，无业务逻辑）

文件：

- `MainActivity.kt`：已有，Flutter host / 启动器入口。
- `GameActivity.kt`：全屏、横屏、独立任务 `singleTask`、`exported=false`、`android:process=":game"`；普通 Activity + SurfaceView，**不继承 FlutterActivity**。
- `GameSurfaceView.kt`：Surface 回调 + `onTouchEvent` 原样透传。
- `GameBridge.kt`：`external fun nativeStart/nativeSurfaceCreated/nativeSurfaceDestroyed/nativeSendInput/nativeStop` 声明。

**GameService 不进首发**：前台 Activity 本身就是前台组件。等「切走再回来游戏还在 / 后台挂机」成为需求，再由前台 GameActivity 拉起 FGS（Android 12+ 不能从后台乱起 FGS）。

账号、下载、版本、参数、内存、渲染器选择、游戏管理——全部不经过 Kotlin。

### 5.7 Rust 的能力边界

Minecraft 本身是 Java，游戏 classpath 上的 **LWJGL Java 类**（GLFW/input/openal 的 Java API）客观上必须存在。三条路径的实质是三种许可后果（详见第八节），必须在 Phase 4 开工前拍板：

1. 路径 A：分发 GPL 的 Java/native 运行时组件 → AML 整体 GPL-3.0。
2. 路径 B：自研 LWJGL Android backend（按上游 SPI）→ 许可干净，但「进主菜单」变成大工程。
3. 内部验证：本机跑、不分发 GPL 组件 → 只证明技术路线，不能发 Release。

不存在「短期用现成运行时 Java 组件快速验证、同时保持自有许可并对外分发」这条路——动态加载 GPL jar 再打进 APK 就是在分发。

渲染翻译层可直接使用宽松许可组件：GL4ES（MIT）、Mesa VirGL/Zink（MIT）。OpenAL Soft 动态链接（LGPL）可接受，不要静态链进 Rust cdylib。

---

## 六、注释与措辞规范（合规敏感）

1. **注释只描述代码自身行为**：做什么、为什么这样做、约束与边界条件。
2. **禁止出现**"移植自 / 复制于 / 参考了某某项目源码 / 抄自"等表述，包括代码注释、文档、commit message、PR 描述。
3. 第三方组件的版权与许可在专门的许可清单（NOTICE / 致谢页）中**如实、完整**列出，不在业务代码注释中夹带来源说明。
4. 自研代码必须是独立实现；参考公开技术资料时，以理解原理后自主编写为原则。
5. 变量、模块命名使用 AML 自有命名体系，不沿用第三方项目的内部命名。
6. 本文档随方案演进更新，历史措辞同步修正。

---

## 七、分阶段计划

| 阶段 | 内容 | 验收 | 状态 |
|---|---|---|---|
| 0 | cargokit aarch64/x86_64，Rust so 进 APK，16KB 对齐，`JNI_OnLoad` 按 cmdline 分流探测 | 主进程 Flutter 正常；logcat 有 Rust 日志 | ✅ |
| 1 | `GameActivity` + `:game` + `taskAffinity`，**不**启动 Flutter engine | 最近任务两张卡片；`ps` 可见两个进程 | ✅ |
| 2 | ART JNI → Rust → `ANativeWindow`；Surface destroy/recreate 不崩 | 旋转/锁屏后能重新 attach | ✅ |
| 3 | `loader.rs` + `libjvm.so` + `JNI_CreateJavaVM` + Hello main；日志走 UDS | 游戏进程里跑的是 **OpenJDK 而非 ART**；启动器可见 Hello 输出 | ✅ |
| 4 | LaunchConfig → Android Adapter → 写清单 → 调 Minecraft main（可先黑屏/崩在 GL） | 游戏主类执行，classpath 被加载 | ✅ |
| 5 | LWJGL 安卓 backend + GL4ES + 绑定 ANativeWindow | **进入 MC 主菜单（路线成立）** | ✅ |
| 6 | 最小触控转视角 | 能进世界操作 | ✅ |
| 7 | Java 21/25、内存策略、Forge/Fabric 回归 | 现代版本 + 整合包可玩 | ✅ JRE21（1.21.1）、Fabric 0.19.5、Forge 52.1.14、NeoForge 21.1.252（2026-09-27，见 7.3） |
| 8 | Zink / VirGL、GPU 黑名单与 fallback | 图形后端韧性 |
| 9 | Boat 路线（LWJGL2）覆盖 1.12 及更早 | 老版本可启动 |
| 10 | 完整输入系统（虚拟鼠标、按键映射、键鼠/手柄） | 触控体验完善 |
| 11 | 多实例：`GameSlotNActivity` + 槽位调度（限高配，按内存/实例类型动态开放） | 两个 MC 同时运行 |
| 12 | FGS 后台保活 | 切走再回来游戏还在 |

阶段 0–5 是核心技术关，主菜单即证明路线成立。**「MC main 跑起来」（阶段 4）与「进主菜单」（阶段 5）必须拆开**：进菜单失败时，要能区分是 JVM 问题、classpath 问题还是 GL 问题。

~~许可拍板须在阶段 4 前完成~~ → 已在 Phase 5 开工前拍板为**内部验证路径**（见第八节第 1 条）；对外发布前需在路径 A/B 间终拍。

### 7.1 实施进度快照（2026-09-25）

| 阶段 | 实际结果（模拟器 x86_64 验收） |
|---|---|
| 0 | `librust_lib_aml.so` 打入 APK 双 ABI；logcat 见 `JNI_OnLoad branch=main`；主进程 Flutter 渲染正常。cargokit 已适配 Flutter 3.44（读 `target-platform` 属性）；reqwest 换 rustls 解决安卓 OpenSSL 交叉编译 |
| 1 | `com.astral.aml` 与 `com.astral.aml:game` 双进程在线；最近任务两张卡片（独立 taskAffinity + singleTask）；`:game` 分支 JNI_OnLoad 日志干净 |
| 2 | `GameBridge`/`GameSurfaceView` 就位；ANativeWindow attach 填色渲染成功；锁屏 detach / 解锁 re-attach 无崩溃。教训：JNI 方法首参须直传 `JNIEnv` 指针；Kotlin external fun 必须放 `object`，不能放 companion |
| 3 | `:game` 进程内 dlopen JRE 树，`JNI_CreateJavaVM` 拉起 **OpenJDK HotSpot 17**（非 ART），Hello main 执行，日志走 UDS。关键根因：boot class path 由 libjvm.so 位置推导，而非 `-Djava.home` |
| 4 | MC 1.20.1 主类 `net/minecraft/client/main/Main` 在 HotSpot 上真实执行，45 条 classpath 生效，推进到渲染层后按计划边界停在 `RenderSystem` 初始化（崩在 GL 属预期）。修复：UDS 日志自死锁、`-cp` 需转 `-Djava.class.path=`、JRE 树缺 `lib/tzdb.dat`（生产 JRE 分发必须包含） |
| 5 | ✅ **2026-09-25 验收通过**：MC 1.20.1 主菜单完整渲染（全景背景、按钮、splash），EGL 上下文建在 Rust 注入的 ANativeWindow 上，GL4ES 翻译 + 着色器编译 + 贴图图集全部工作。联调修复两处：`pojavNotifyWindow` 在 `br_setup_window` 未初始化（pojavInit 前 surface 先 attach）时空调用 → SIGSEGV，已加 NULL 保护（上游 setupBridgeWindow 本有此保护，裁剪时漏掉）；HotSpot `System.loadLibrary("pojavexec")` 只搜 `java.library.path`，需把 `libpojavexec.so` 同时放进 natives 目录（双副本靠 `POJAV_ENVIRON` env 共享状态）。另补 `bridge_window.c` 缺 `stdlib.h`（getenv）。已知噪音：LWJGL 3.3.3-snapshot jar 与预编译 natives 版本签名告警（非致命） |
| 6 | ✅ **2026-09-26 验收通过**：主菜单点按（单人游戏 → 创建新的世界）→ 进世界 → 滑动转视角（画面明显旋转）→ 返回键=ESC 打开暂停菜单（grab 自动解除）→ 菜单点按「回到游戏」恢复抓取，全链路无崩溃。实现：`GameSurfaceView.onTouchEvent` 按 `CallbackBridge.isGrabbing()` 分流——非抓取=虚拟光标（绝对定位，点按=左键点按、按住超 slop=左键拖拽），抓取=相对增量驱动无界虚拟光标（×0.5 灵敏度），事件直送 pojavexec RegisterNatives 入口（无 Rust 往返）；`surfaceChanged` 补送 `nativeSendScreenSize`（此前缺调用，顺带修正了窗口逻辑尺寸）；`GameActivity` 拦截返回键映射 `GLFW_KEY_ESCAPE`。**关键 bug**：裁剪上游 `input_bridge_v3.c` 时，`send_*` 系列函数漏声明 `(JNIEnv*, jclass)` 前缀——经 RegisterNatives 注册的函数 JNI ABI 照样前送这两个参数，缺失导致参数整体错位，首次点击即 SIGSEGV（`send_mouse_button` 把 env 指针截断当按钮序号索引 `mouseDownBuffer`）。另：旧 `:game` 进程存活时 `LAUNCH_SINGLE_TASK` 拉起 `GameActivity` 不走 `onCreate`，新 Intent 需 `onNewIntent` 承接（重启语义 Phase 12 前再定） |

Phase 5 拆为三个可验收小步：**p5-artifacts**（natives + stub jar 部署到设备 `components/lwjgl3/`）→ **p5-classpath**（stub jar 前置 + LWJGL/GL4ES 属性与 env 经清单传递）→ **p5-window**（EGL 桥双 VM 注册 + ANativeWindow 注入）。三步均已落地并通过端到端验收。

### 7.2 Phase 7 进度快照（2026-09-27，模拟器 x86_64）

**验收结果**：MC 1.21.1（JRE21）进标题、建世界、触控转视角全链路通过；1.20.1（JRE17）进世界 + 转视角回归通过（懒加载桥在系统 loader 与旧运行时均正常）；**Fabric Loader 0.19.5 + 1.21.1 进标题、建世界、触控转视角通过**（游戏类均在 `knot//` 类加载器下执行，日志零 `already loaded`/`Failed to locate library`；唯一噪音为既有无害 `libflite.so` Narrator 缺失）。

关键修复（均有实证）：

1. **JRE21 `NoSuchFieldError: preferIPv6Address`（linker namespace 同名库冲突）**。App linker namespace `clns-9` 的搜索路径含 APK `nativeLibraryDir`（整套 JDK17 .so）但**不含** `<jre>/lib`；RTLD_GLOBAL dlopen `<jre21>/lib/libnio.so` 时，Bionic 按 SONAME 解析 `DT_NEEDED libnet.so` 命中 APK 内旧 JDK17 版（含 `preferIPv6Address` 字段），而 Java 类是 JDK21 布局 → 崩。修复：`loader.rs` 在 `JNI_CreateJavaVM` 前按依赖顺序用**绝对路径 RTLD_GLOBAL** 预加载 JDK 核心库（常量 `JDK_PRELOAD`：libverify/libjava/libzip/libjimage/libnet/libnio/libextnet/…，约 19 个；文件不存在跳过、加载失败仅 log）。刻意**排除 AWT/Swing/font**——libfreetype 必须继续解析到 LWJGL 桥接版。注意先捕获 `let jvm = *handles.last().unwrap()` 再 preload（handle 也进同一 vec）。
2. **Fabric `UnsatisfiedLinkError: Native Library liblwjgl.so already loaded in another classloader`（双 loader 初始化冲突）**。Fabric Knot 用自己的 URLClassLoader 加载 LWJGL；此前 `pojav_bridge_on_runtime()` 在 `CreateJavaVM` 后立即（裸线程、系统 loader）`FindClass("org/lwjgl/glfw/GLFW")` 并读取 `keyDownBuffer/mouseDownBuffer` 静态字段——**读静态字段会触发类初始化**，使 stub GLFW 及 LWJGL 核心 native 绑定在系统 loader；Knot 游戏 loader 再加载即被 JDK 拒绝。修复（`input_bridge_v3.c`）：`pojav_bridge_on_runtime` 只缓存 `JavaVM*`；新增 `pojav_ensure_runtime_classes(JNIEnv*)`（once guard，失败可重试），在 `pojavInit()`（stub `glfwInit()` 的 Java 帧内，env 沿帧找到正确 loader——vanilla=系统、Fabric=Knot）里首次解析 GLFW 类/方法/buffer；`pojavPumpEvents`、`nativeSetWindowAttrib` 入口补 ensure。FindClass/GetStaticMethodID/GetStaticFieldID 不初始化类，只有 GetStaticObjectField 会。
3. **LWJGL 库定位**：JVM 参数加 `-Dorg.lwjgl.librarypath=<natives>`（与 `-Djna.boot.library.path` 并列），Knot loader 下 LWJGL 按绝对路径 dlopen，不再依赖 `findLibrary()` 回退。
4. **native staging 产品化**：新建 `rust/src/android/stage.rs`（`ensure_bridge_natives()`：解析 `/proc/self/maps` 定位 APK nativeLibraryDir，按 size 把 10 个桥接库复制进实例 natives；注意 maps 匿名行无第 6 字段，`nth(5)` 必须 `let Some else continue`），由 `install.rs` 创建 natives 目录后调用。AGP 会 strip jniLibs 产物（体积变小，功能正常）。
5. **桥接库进 jniLibs（x86_64）+ arm64 libjnidispatch**：gl4es/openal/lwjgl*/pojavexec/Bionic JNA 7.0.0（自编译，官方 jar 内是 glibc）入包；libfreetype 必须用 LWJGL 版（795216 B）而非 AWT 版（SONAME 相同）。arm64 仅 libjnidispatch 就绪，其余桥接库待供应。
6. **pojavexec 构建**：flutter/cargokit 只构建根包，`pojavexec` crate 需手动 `cargo build -p pojavexec --target <triple>`（`CC` 必须显式指向 NDK clang wrapper，build.rs 只读 `CC` 不读 `CC_<target>`；API 26，r28c），产物在 `target/<triple>/debug/build/pojavexec-*/out/libpojavexec.so`，手动复制进 `jniLibs/<abi>/` 后重新打包。已给 build.rs 补 `cargo:rerun-if-changed=c/<src>`，C 改动不再被 cargo 缓存吞掉。

### 7.3 Phase 7 续：Forge 回归快照（2026-09-27，模拟器 x86_64）

**验收结果**：Forge 1.21.1-52.1.14 端到端通过——标题界面（"Forge 52.1.14，2 个 Mod 已加载"）→ 单人游戏 → 创建新的世界 → 世界生成 → 进世界第一人称渲染完整。回归未受影响：Vanilla 1.21.1 标题 ✓、Fabric 1.21.1 标题 ✓、Vanilla 1.20.1（JRE17）标题 ✓（均走旧 stub 前置路径）。

**安装链路（f1–f6）**：Forge/NeoForge 的 install_profile.json 带 `processors`（下载 jar、解 lzma、重生成 client/slim jar），桌面版是 spawn 外部 `java` 进程；安卓没有可 exec 的 java，改为 **`:proc` 独立进程内嵌 HotSpot 跑 processor**：`rust/src/android/processor.rs` 复用 launch 清单（headless 模式——`jvm.rs` 跳过 pojav native 注册与 surface 等待，`chdir` 到实例目录），主进程靠 `JNI_OnLoad` 捕获的 ART `JavaVM*`（`mod.rs::MAIN_ART_VM`）直接 `StartService` 拉起 `ProcessorService`（:proc，AndroidManifest 已声明）。`install.rs` 按 `#[cfg(target_os)]` 分流；processor 的 JRE 用 `runtime/jre::select` 按版本所需 major 选取。数据解析沿用既有修复：**AML 自算的绝对路径（MINECRAFT_JAR/ROOT/LIBRARY_DIR）不进 resolve_data_value**，否则被当作 Forge 的 `/relative` 语法二次拼前缀。

**JPMS 冲突与逐模块重打包（f7 核心，新文件 `rust/src/android/lwjgl_repack.rs`）**：

- 根因：Forge ≥1.21 由 `net.minecraftforge.bootstrap.ForgeBootstrap` 启动，`SecureModuleFinder.of(...)` 把 `java.class.path` 的**每个 jar 都变 JPMS 模块**。胖 stub `lwjgl-glfw-classes.jar` 无 `Automatic-Module-Name` → 按文件名派生自动模块 `lwjgl.glfw.classes` 导出全部包，与真 `org.lwjgl.glfw` 显式模块（MR jar，module-info 在 `META-INF/versions/9`）包导出冲突 → `ResolutionException: Modules lwjgl.glfw.classes and org.lwjgl.glfw export package org.lwjgl.glfw ...`。
- 方案：**不再整个 stub jar 前置**，而是逐模块重打包——复制每个真 org.lwjgl jar 的全部条目（MANIFEST/module-info 原样保留），类被 stub 覆盖的用 stub 字节替换、模块内缺失的 stub 类增补进去；**`android/util` 折叠进 glfw 模块 jar**（全 stub 仅 `org.lwjgl.glfw.GLFW` 引用它，放 extras 会因模块不互相 read 而 `IllegalAccessError`）；剩余孤儿包（javax/annotation、org/lwjgl/input、util/*、nanovg 等 10 个）进一个补充自动模块 `lwjgl-android-extras.jar`。
- 触发门控 `is_modular_loader_main`（main_class 含 `minecraftforge.bootstrap`/`bootstraplauncher`/`neoforged`），仅模块化加载器走 repack，vanilla/Fabric 路径不变；NeoForge 已由同机制端到端验证。
- 缓存：`files/components/lwjgl3/patched/*.jar` + `.sig` sidecar，sig 含 `REPACK_VERSION=2` 与输入文件 len+mtime（repack 逻辑变更必须 bump 版本号，否则设备缓存不失效）。
- 测试注意：`mod android` 被 `#[cfg(target_os="android")]` 门控，host `cargo test` 跑不到该模块测试；本地验证用 scratch crate（复制源码 + 替换 `classpath_separator`）跑 fixture 断言。

**NeoForge 端到端（2026-09-27 补验）**：NeoForge 1.21.1-21.1.252 全流程通过——实例创建 → 安装（10 个 processors 经 :proc 进程）→ 启动 → 标题界面（模组按钮可见）→ 创建新的世界 → 世界生成 → 进世界第一人称渲染。两处新修复：

1. **JNI_CreateJavaVM 双元素选项（rc=-1 根因）**：NeoForge version JSON 的 JVM args 以**两元素分离形式**携带 `-p <module-path>`、`--add-modules ALL-MODULE-PATH`、`--add-opens java.base/...=<mod>`、`--add-exports ...`。`JNI_CreateJavaVM` 逐个处理 option 字符串、不会把下一个元素当值消费，未识别的裸字符串直接 rc=-1。修复（`launch.rs` `build_manifest`）：通用化合并——`VALUE_OPTS` 列表（`-p`/`--module-path`/`--add-modules`/`--add-opens`/`--add-exports`/`--add-reads`/`--patch-module`/`--limit-modules`）遇到即消费下一元素合并为 `--opt=value`（`-p` 归一为 `--module-path`）。
2. **安装中断残留 `.amlpart`**：`http_download.rs` 下载经 `.amlpart` 临时文件（sha1 校验后 rename），模拟器崩溃中断后残留导致 processor jar 缺失（`read_jar_main_class` ENOENT → "No such file or directory (os error 2)"）。点「重试」重新安装即补完，无需额外处理。

另：模拟器稳定性经验——NVIDIA 混合显卡 Wayland 主机上 `-gpu swiftshader_indirect` 反复 SIGSEGV（qemu-system-x86_64 core dump），必须 `-gpu host` + `pm disable com.google.android.apps.wellbeing`（Digital Wellbeing ANR 弹窗是崩溃表象之一）。

---

## 八、风险与注意点

1. **许可证决策（阶段 4 前拍板，最高优先）**：PojavLauncher、Boat 系运行时项目为 GPL-3.0。若复用其代码或预编译产物，AML 整体须以 GPL-3.0 开源并提供完整对应源码。AML 仓库**目前没有 LICENSE 文件**。三条路径：
   - A：AML 整体 GPL-3.0，分发 GPL 运行时组件；
   - B：自研 LWJGL Android backend（仅用 MIT/BSD/BSD-3 组件：GL4ES、Mesa、ANGLE），许可干净但工期大增；
   - 内部验证：本机跑不分发 GPL 组件，只证路线、不能发 Release。
   按「源码 / jar / so / 静态链接 / 动态链接 / 构建脚本」逐项清点，维护 NOTICE / THIRD_PARTY_LICENSES，而不是一句话定性。

   **已拍板（2026-09，Phase 5 开工前）：走「内部验证」路径**——本地引入开源组件（含 GPL 的 GLFW stub jar、EGL 桥 C 实现与预编译 natives，外加 OpenAL Soft），仅本机/模拟器验证，**不分发、不发 Release**。含义：Phase 5 代码与产物不进任何对外构建；将来要对外发布时，必须在路径 A（整体 GPL-3.0）与路径 B（自研替换 GPL 组件）之间终拍，并先清理仓库中的 GPL 工件。
2. **从 `libjli` 取 `JNI_CreateJavaVM`（事实错误）**：必须从 `libjvm.so`（`lib/server/libjvm.so`）取，路径由 RuntimeLayout 描述。
3. **ART JNIEnv 漏进游戏 VM**：两套 VM 严格隔离；ffi 只做转换，游戏逻辑只用 HotSpot env。
4. **在 UI 线程调 `JNI_CreateJavaVM`**：必须在专门游戏线程，否则 UI 线程变 MC 主线程直接 ANR。
5. **`dup2(1/2)` 破坏 ART 日志**：不重定向进程级 stdout，改游戏 VM 内 System.out→UDS / log4j appender。
6. **JRE DT_NEEDED / namespace 加载失败**：`loader.rs` 绝对路径 + 依赖顺序加载 + 失败日志。
7. **信号冲突 ART × HotSpot**：先加载 `libjsig.so`。
8. **16KB 页 vs 旧 JRE 产物**：Android 15+ 强制；`vendor/jre.rs` 解压后做对齐检查，不合格拒收或自己重建。
9. **FRB/Flutter 在 `:game` 初始化**：`JNI_OnLoad` 按 cmdline 分流；GameActivity 非 Flutter。
10. **panic 边界**：panic unwind 跨 JNI 是 UB，所有 `Java_*` 入口 `catch_unwind`。
11. **W^X 执行限制**：以 jniLibs 打包或纯 dlopen，不从数据目录 exec。
12. **GameActivity `exported=false`**：清单路径校验在私有目录内，防 classpath 注入 ≈ 任意代码执行。
13. **`-Xmx` 触发 LMK**：Backend 按 `ActivityManager.MemoryInfo`（availMem/totalMem）写保守上限。
14. **线程 attach**：跨线程回调 Kotlin 时用 ART 的 `JavaVM*`（JNI_OnLoad 存的），`AttachCurrentThread`，用完 detach。
15. **GPU 驱动碎片化**：首发只承诺 GL4ES；Zink/VirGL 按安卓语义理解（非桌面/QEMU 那套），后期再做黑名单。
16. **仅做 64 位**：arm64＋x86_64（模拟器）。
17. **FRB codegen 约束沿用**：新增 android API 后跑 codegen；避免 `std::pin::pin!`（用 `Box::pin`）；生成物禁止手改；codegen 需完整 JAVA_HOME/GRADLE_USER_HOME + rustfmt 环境。
18. **`jni`/`ndk` crate API 按实际版本核对**：`EnvUnowned`、`inner_ptr()`、`ptr_from_surface` 等名字可能不存在，Phase 0 先做最小探测。

---

## 九、参考资料

### 运行时与 JRE

- PojavLauncher（已于 2025-09 归档，仅作原理参考）：https://github.com/PojavLauncherTeam/PojavLauncher
- BoatApp：https://github.com/Cosinemath/BoatApp
- MCinaBox（Boat 路线衍生）：https://github.com/AOF-Dev/MCinaBox
- 安卓 OpenJDK 构建：https://github.com/PojavLauncherTeam/android-openjdk-build-multiarch
- LWJGLX（LWJGL2→3 兼容层）：https://github.com/PojavLauncherTeam/lwjglx

### 渲染器与图形

- GL4ES：https://github.com/ptitSeb/gl4es
- Mesa（VirGL / Zink）：https://gitlab.freedesktop.org/mesa/mesa
- ANGLE：https://github.com/google/angle

### Rust 工具链

- jni crate 文档：https://docs.rs/jni/latest/jni/
- ndk crate：https://docs.rs/ndk/
- android-activity（仅作技术参考，本方案不使用）：https://docs.rs/android-activity/latest/android_activity/
- AOSP Android Rust 模式：https://source.android.google.cn/docs/setup/build/rust/building-rust-modules/android-rust-patterns

### Android 平台规范

- 16KB 页大小支持：https://developer.android.com/guide/practices/page-sizes
- Activity 元素（android:process / taskAffinity / launchMode / documentLaunchMode）：https://developer.android.com/guide/topics/manifest/activity-element
