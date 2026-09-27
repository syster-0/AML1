import 'dart:async';
import 'dart:convert';

import 'package:aml/src/app/di/service_locator.dart';
import 'package:aml/src/features/instances/application/instance_store.dart';
import 'package:aml/src/features/java/application/java_download_service.dart';
import 'package:aml/src/rust/api/launcher.dart' as rust;
import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart'
    show FocusNode, TextEditingValue, TextEditingController, TextSelection;

/// 实例设置对话框的共享表单状态机。
///
/// 持有全部输入控制器、override 开关、默认值与保存编排（防抖 + 18 参
/// 增量保存），四个设置标签页（通用 / 窗口 / Java / Hooks）共用。
class InstanceSettingsController extends ChangeNotifier {
  InstanceSettingsController({required this.instanceId});

  final String instanceId;
  final _store = getIt<InstanceStore>();

  final nameController = TextEditingController();
  final nameFocusNode = FocusNode();
  final widthController = TextEditingController(text: '854');
  final heightController = TextEditingController(text: '480');
  final jvmArgsController = TextEditingController();
  final envVarsController = TextEditingController();
  final jvmArgsFocusNode = FocusNode();
  final envVarsFocusNode = FocusNode();
  final preLaunchController = TextEditingController();
  final wrapperController = TextEditingController();
  final postExitController = TextEditingController();
  final groupController = TextEditingController();

  Timer? _saveDebounce;
  int requiredJavaMajor = 21;
  int maxMemoryMb = 8192;
  double memoryMb = 4096;
  String javaPath = '';
  String defaultJavaPath = '';
  int defaultMemoryMb = 4096;
  String defaultJvmArgs = '';
  String defaultEnvVars = '';
  int defaultWindowWidth = 854;
  int defaultWindowHeight = 480;
  bool defaultFullscreen = false;
  String updateChannel = 'release';
  List<String> groups = [];
  List<String> availableGroups = [];
  bool overrideWindow = false;
  bool overrideJava = false;
  bool overrideMemory = false;
  bool overrideJvmArgs = false;
  bool overrideEnvVars = false;
  bool overrideHooks = false;
  bool useSystemGlfw = false;
  bool useSystemOpenal = false;
  bool fullscreen = false;
  bool saving = false;
  String? error;
  String _lastCommittedName = '';
  bool _disposed = false;

  /// 串行化所有保存请求，避免 `saving` 期间静默丢弃最后一次编辑。
  Future<void> _saveChain = Future<void>.value();

  rust.InstanceDto? get instance {
    for (final item in _store.instances.value) {
      if (item.id == instanceId) return item;
    }
    return null;
  }

  @override
  void dispose() {
    _disposed = true;
    _saveDebounce?.cancel();
    nameController.dispose();
    nameFocusNode.dispose();
    widthController.dispose();
    heightController.dispose();
    jvmArgsController.dispose();
    envVarsController.dispose();
    jvmArgsFocusNode.dispose();
    envVarsFocusNode.dispose();
    preLaunchController.dispose();
    wrapperController.dispose();
    postExitController.dispose();
    groupController.dispose();
    super.dispose();
  }

  Future<void> loadMeta() async {
    try {
      final requiredMajor = await rust.getRequiredJavaVersion(id: instanceId);
      final maxMemory = await getIt<JavaDownloadService>().getMaxMemory();
      final allGroups = await _store.listAllGroups();
      final defaults = await _store.getLaunchDefaults();
      final optimalJava = await _store.peekJavaForMajor(requiredMajor);
      if (_disposed) return;
      requiredJavaMajor = requiredMajor;
      maxMemoryMb = maxMemory.clamp(512, 131072);
      availableGroups = allGroups;
      defaultMemoryMb = defaults.memoryMb.toInt().clamp(512, maxMemoryMb);
      defaultJvmArgs = defaults.extraJvmArgs ?? '';
      defaultEnvVars = envVarsToDisplay(defaults.environmentVars);
      defaultWindowWidth = defaults.windowWidth.toInt();
      defaultWindowHeight = defaults.windowHeight.toInt();
      defaultFullscreen = defaults.fullscreen;
      defaultJavaPath = optimalJava ?? '';
    } catch (_) {}
    if (!_disposed) {
      syncFromInstance(instance);
      notifyListeners();
    }
  }

  /// 用实例当前值刷新表单；名称字段聚焦时不打断输入。
  void syncFromInstance(rust.InstanceDto? instance) {
    if (instance == null) return;
    if (!nameFocusNode.hasFocus) {
      nameController.text = instance.name;
      _lastCommittedName = instance.name;
    }
    overrideJava = instance.javaPath != null && instance.javaPath!.isNotEmpty;
    javaPath = overrideJava ? (instance.javaPath ?? '') : defaultJavaPath;
    overrideMemory = instance.memoryMb != null;
    memoryMb = (instance.memoryMb?.toInt() ?? defaultMemoryMb)
        .clamp(512, maxMemoryMb)
        .toDouble();
    overrideWindow = instance.windowWidth != null ||
        instance.windowHeight != null ||
        instance.fullscreen != null;
    widthController.text =
        '${instance.windowWidth?.toInt() ?? defaultWindowWidth}';
    heightController.text =
        '${instance.windowHeight?.toInt() ?? defaultWindowHeight}';
    fullscreen = instance.fullscreen ?? defaultFullscreen;
    // 输入框聚焦时（正在编辑）不要用服务端值覆盖文本/开关，否则光标跳变、
    // 输入到一半的内容被还原，开关也会误弹回。
    if (!jvmArgsFocusNode.hasFocus) {
      overrideJvmArgs = instance.extraJvmArgs != null;
      jvmArgsController.text =
          overrideJvmArgs ? (instance.extraJvmArgs ?? '') : defaultJvmArgs;
    }
    final envJson = instance.environmentVars;
    if (envJson != null && envJson.trim().isNotEmpty) {
      try {
        final map = jsonDecode(envJson) as Map<String, dynamic>;
        useSystemGlfw = map['AML_SYSTEM_GLFW'] == '1' || map['AML_SYSTEM_GLFW'] == 'true';
        useSystemOpenal = map['AML_SYSTEM_OPENAL'] == '1' || map['AML_SYSTEM_OPENAL'] == 'true';
      } catch (_) {
        useSystemGlfw = false;
        useSystemOpenal = false;
      }
    } else {
      useSystemGlfw = false;
      useSystemOpenal = false;
    }
    if (!envVarsFocusNode.hasFocus) {
      final envDisplay = envVarsToDisplay(instance.environmentVars);
      overrideEnvVars = instance.environmentVars != null && envDisplay.trim().isNotEmpty;
      envVarsController.text = overrideEnvVars
          ? envDisplay
          : defaultEnvVars;
    }
    preLaunchController.text = instance.preLaunchCommand ?? '';
    wrapperController.text = instance.wrapperCommand ?? '';
    postExitController.text = instance.postExitCommand ?? '';
    updateChannel = instance.updateChannel;
    groups = List<String>.from(instance.groups);
    overrideHooks = _hasHook(instance.preLaunchCommand) ||
        _hasHook(instance.wrapperCommand) ||
        _hasHook(instance.postExitCommand);
    notifyListeners();
  }

  bool _hasHook(String? value) => value != null && value.trim().isNotEmpty;

  String envVarsToDisplay(String? json) {
    if (json == null || json.trim().isEmpty) return '';
    try {
      final map = jsonDecode(json) as Map<String, dynamic>;
      return map.entries
          .where((entry) => entry.key != 'AML_SYSTEM_GLFW' && entry.key != 'AML_SYSTEM_OPENAL')
          .map((entry) => '${entry.key}=${entry.value}')
          .join('\n');
    } catch (_) {
      return json;
    }
  }

  String? envVarsToJson(String text) {
    final trimmed = text.trim();
    final map = <String, String>{};
    if (trimmed.isNotEmpty) {
      for (final line in trimmed.split('\n')) {
        final item = line.trim();
        if (item.isEmpty) continue;
        final index = item.indexOf('=');
        if (index <= 0) continue;
        final k = item.substring(0, index).trim();
        final v = item.substring(index + 1).trim();
        if (k != 'AML_SYSTEM_GLFW' && k != 'AML_SYSTEM_OPENAL') {
          map[k] = v;
        }
      }
    }
    if (useSystemGlfw) {
      map['AML_SYSTEM_GLFW'] = '1';
    }
    if (useSystemOpenal) {
      map['AML_SYSTEM_OPENAL'] = '1';
    }
    if (map.isEmpty) return null;
    return jsonEncode(map);
  }

  /// 提交名称编辑（清洗非法字符并保存）。
  Future<void> commitName() async {
    final raw = nameController.text.trim();
    final name = raw.replaceAll(RegExp(r'[\\/:*?"<>|]'), '_').trim();
    if (name.isEmpty || name == _lastCommittedName) {
      if (name != raw && !_disposed) {
        nameController.text = name;
      }
      return;
    }
    if (name == (instance?.name ?? '')) {
      _lastCommittedName = name;
      if (name != raw && !_disposed) {
        nameController.value = TextEditingValue(
          text: name,
          selection: TextSelection.collapsed(offset: name.length),
        );
      }
      return;
    }
    await save(name: name);
    if (!_disposed) {
      _lastCommittedName = name;
      if (name != raw) {
        nameController.value = TextEditingValue(
          text: name,
          selection: TextSelection.collapsed(offset: name.length),
        );
      }
    }
  }

  void scheduleSave(Future<void> Function() action) {
    _saveDebounce?.cancel();
    _saveDebounce = Timer(const Duration(milliseconds: 450), () {
      unawaited(action());
    });
  }

  /// JVM 参数输入防抖保存：清空时回退为清除覆盖（沿用全局默认值）。
  void scheduleJvmArgsSave(String value) {
    final trimmed = value.trim();
    scheduleSave(
      () => save(
        extraJvmArgs: trimmed.isEmpty ? null : value,
        clearExtraJvmArgs: trimmed.isEmpty,
      ),
    );
  }

  /// 环境变量输入防抖保存。尚未形成完整 KEY=VALUE 的中间状态不持久化，
  /// 以免服务端回显把正在输入的内容抹掉；整框清空则清除覆盖。
  void scheduleEnvVarsSave(String value) {
    final trimmed = value.trim();
    if (trimmed.isEmpty) {
      scheduleSave(() => save(clearEnvironmentVars: true));
      return;
    }
    final json = envVarsToJson(value);
    if (json == null) return;
    scheduleSave(() => save(environmentVars: json));
  }

  // ---- 领域更新方法：改字段 + 通知 + 持久化，供各标签页调用 ----

  Future<void> setOverrideWindow(bool enabled) async {
    overrideWindow = enabled;
    if (!enabled) {
      widthController.text = '$defaultWindowWidth';
      heightController.text = '$defaultWindowHeight';
      fullscreen = defaultFullscreen;
    }
    notifyListeners();
    if (!enabled) {
      await save(clearWindowSettings: true);
    } else {
      await save(
        windowWidth: int.tryParse(widthController.text) ?? defaultWindowWidth,
        windowHeight:
            int.tryParse(heightController.text) ?? defaultWindowHeight,
        fullscreen: fullscreen,
      );
    }
  }

  Future<void> setFullscreen(bool value) async {
    fullscreen = value;
    notifyListeners();
    await save(fullscreen: value);
  }

  Future<void> setOverrideJava(bool enabled) async {
    overrideJava = enabled;
    if (enabled && javaPath.isEmpty) {
      javaPath = defaultJavaPath;
    }
    if (!enabled) {
      javaPath = defaultJavaPath;
    }
    notifyListeners();
    if (!enabled) {
      await save(clearJavaPath: true);
    } else if (javaPath.isNotEmpty) {
      await save(javaPath: javaPath);
    }
  }

  void setJavaPath(String path) {
    javaPath = path;
    notifyListeners();
    scheduleSave(() => save(javaPath: path));
  }

  Future<void> setOverrideMemory(bool enabled) async {
    overrideMemory = enabled;
    if (!enabled) {
      memoryMb = defaultMemoryMb.toDouble();
    }
    notifyListeners();
    if (!enabled) {
      await save(clearMemoryMb: true);
    } else {
      await save(memoryMb: memoryMb.round());
    }
  }

  void setMemoryMb(double value) {
    memoryMb = value;
    notifyListeners();
    scheduleSave(() => save(memoryMb: value.round()));
  }

  Future<void> setOverrideJvmArgs(bool enabled) async {
    overrideJvmArgs = enabled;
    if (enabled && jvmArgsController.text.trim().isEmpty) {
      jvmArgsController.text = defaultJvmArgs;
    }
    if (!enabled) {
      jvmArgsController.text = defaultJvmArgs;
    }
    notifyListeners();
    if (!enabled) {
      await save(clearExtraJvmArgs: true);
    } else if (jvmArgsController.text.trim().isNotEmpty) {
      // 空白内容不持久化，避免空串覆盖全局默认值并导致开关状态来回弹。
      await save(extraJvmArgs: jvmArgsController.text);
    }
  }

  Future<void> setUseSystemGlfw(bool enabled) async {
    useSystemGlfw = enabled;
    notifyListeners();
    await _saveSystemLibraries();
  }

  Future<void> setUseSystemOpenal(bool enabled) async {
    useSystemOpenal = enabled;
    notifyListeners();
    await _saveSystemLibraries();
  }

  Future<void> _saveSystemLibraries() async {
    final json = envVarsToJson(envVarsController.text);
    if (json == null) {
      await save(clearEnvironmentVars: true);
    } else {
      await save(environmentVars: json);
    }
  }

  Future<void> setOverrideEnvVars(bool enabled) async {
    overrideEnvVars = enabled;
    if (enabled && envVarsController.text.trim().isEmpty) {
      envVarsController.text = defaultEnvVars;
    }
    if (!enabled) {
      envVarsController.text = defaultEnvVars;
    }
    notifyListeners();
    if (!enabled) {
      if (useSystemGlfw || useSystemOpenal) {
        final map = <String, String>{};
        if (useSystemGlfw) map['AML_SYSTEM_GLFW'] = '1';
        if (useSystemOpenal) map['AML_SYSTEM_OPENAL'] = '1';
        await save(environmentVars: jsonEncode(map));
      } else {
        await save(clearEnvironmentVars: true);
      }
    } else {
      final json = envVarsToJson(envVarsController.text);
      if (json != null) {
        await save(environmentVars: json);
      }
    }
  }

  Future<void> setOverrideHooks(bool enabled) async {
    overrideHooks = enabled;
    notifyListeners();
    if (!enabled) {
      await save(clearHooks: true);
    } else {
      await save(
        preLaunchCommand: preLaunchController.text,
        wrapperCommand: wrapperController.text,
        postExitCommand: postExitController.text,
      );
    }
  }

  void setUpdateChannel(String value) {
    updateChannel = value;
    notifyListeners();
    unawaited(save(updateChannel: value));
  }

  Future<void> save({
    String? name,
    String? javaPath,
    bool clearJavaPath = false,
    int? memoryMb,
    bool clearMemoryMb = false,
    String? extraJvmArgs,
    bool clearExtraJvmArgs = false,
    int? windowWidth,
    int? windowHeight,
    bool? fullscreen,
    bool clearWindowSettings = false,
    String? environmentVars,
    bool clearEnvironmentVars = false,
    String? preLaunchCommand,
    String? wrapperCommand,
    String? postExitCommand,
    bool clearHooks = false,
    String? updateChannel,
  }) {
    // 串行排队：上一次保存未完成时等待，而不是静默丢弃本次编辑。
    final completer = Completer<void>();
    final previous = _saveChain;
    _saveChain = completer.future;
    unawaited(
      previous.then((_) async {
        if (_disposed) {
          completer.complete();
          return;
        }
        saving = true;
        error = null;
        notifyListeners();
        try {
          await _store.updateSettings(
            id: instanceId,
            name: name,
            javaPath: javaPath,
            clearJavaPath: clearJavaPath,
            memoryMb: memoryMb,
            clearMemoryMb: clearMemoryMb,
            extraJvmArgs: extraJvmArgs,
            clearExtraJvmArgs: clearExtraJvmArgs,
            windowWidth: windowWidth,
            windowHeight: windowHeight,
            fullscreen: fullscreen,
            clearWindowSettings: clearWindowSettings,
            environmentVars: environmentVars,
            clearEnvironmentVars: clearEnvironmentVars,
            preLaunchCommand: preLaunchCommand,
            wrapperCommand: wrapperCommand,
            postExitCommand: postExitCommand,
            clearHooks: clearHooks,
            updateChannel: updateChannel,
          );
          syncFromInstance(instance);
        } catch (e) {
          error = '$e';
        } finally {
          if (!_disposed) {
            saving = false;
            notifyListeners();
          }
          completer.complete();
        }
      }),
    );
    return completer.future;
  }

  Future<void> persistGroups(List<String> next) async {
    groups = next;
    saving = true;
    notifyListeners();
    try {
      await _store.setGroups(instanceId, next);
      final all = await _store.listAllGroups();
      if (_disposed) return;
      availableGroups = all;
      syncFromInstance(instance);
    } catch (e) {
      if (!_disposed) error = '$e';
    } finally {
      if (!_disposed) {
        saving = false;
        notifyListeners();
      }
    }
  }
}
