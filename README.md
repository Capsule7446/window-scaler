# 窗口缩放器

Windows独立工具，使用Rust + GPUI Kit。直接修改目标程序的原窗口尺寸。

## 使用

1. 双击 `WindowScaler.exe`，搜索并勾选需要调整的窗口。
2. 输入目标内容区宽度（默认240像素），点击“缩小”。“放大”先尝试系统最大化；仍未实际变大时，恢复本次运行第一次缩小前的原尺寸。
3. 关闭主界面后，左键单击托盘图标可重新打开；右键选择退出，会先放大本次缩过的窗口。

过滤后仍保留勾选：按钮会操作全部已选窗口，包括被搜索条件隐藏的窗口。界面显示隐藏的已选数量。

仅保存宽度，配置位于 `%LOCALAPPDATA%\WindowScaler\settings.json`。工具重开后搜索和勾选清空。

原尺寸记录只在本次运行有效；若窗口在工具启动前就已经很小，工具不能推断之前的大小。回退成功会显示“已恢复原尺寸”，没有记录或恢复失败时会保留错误提示。

## 兼容范围

- 软件自行决定缩小后的内容布局，可能重排或裁切。工具不创建镜像、不修改游戏内鼠标坐标，也不承诺降低CPU、GPU或内存。
- 无响应或权限不足的目标会显示失败，其他目标继续处理。窗口身份校验使用本次运行专用的临时窗口属性；窗口销毁后属性随之清除，避免句柄复用时误操作。
- 退出放大失败时可重试，或选择“仍然退出”。强制结束、崩溃和系统关机不能保证完成放大。
- 窗口操作在各自独立的工作线程顺序执行；结果不明确时不会向同一窗口继续提交操作。

## 构建

需要Rust 1.98.0 MSVC、Visual Studio 2022 Build Tools的C++工具/Windows SDK/CMake。构建脚本自动读取Visual Studio编译环境。

```powershell
.\build.ps1 -Mode test
.\build.ps1 -Mode clippy
.\build.ps1 -Mode desktop-test
.\build.ps1 -Mode release
```

产物为 `dist\WindowScaler.exe`。依赖通过 `Cargo.lock` 固定。

`desktop-test` 需要已登录的 Windows 桌面，在独立进程中创建专用窗口并运行真实 GPUI 与托盘消息；`desktop-test-release` 验证优化构建。它们不向游戏或其他应用发送键鼠输入，测试设置与正常设置隔离。普通 `test` 不运行桌面验收，也不操作游戏。

`build.ps1 -Mode permission-test` 是独立的权限拒绝验收，需要当前进程具备创建受限令牌子进程的权限；仅降低专用测试子进程的完整性，不修改 UAC 设置，也不为工具提权。默认桌面验收不包含这一环境条件。

## 项目与任务

- 代码仓库：[Capsule7446/window-scaler](https://github.com/Capsule7446/window-scaler)
- Ticket跟踪：[GitHub Issues](https://github.com/Capsule7446/window-scaler/issues)
- [已确认规格](docs/spec.md) · [验证记录](docs/validation.md) · [任务跟踪规则](docs/agents/issue-tracker.md)
- [分支与发布流程](docs/branching.md)：`feature → epic → release → master`；`hotfix`负责修复。当前大功能为 [Epic #5](https://github.com/Capsule7446/window-scaler/issues/5)。

`release/vX.Y.Z`会自动测试打包，合入master后由GitHub Actions自动打tag并发布ZIP、EXE和校验文件；正式包见 [GitHub Releases](https://github.com/Capsule7446/window-scaler/releases)。

当前测试证据与剩余环境限制见[验证记录](docs/validation.md)。正式发布由[版本验收 #4](https://github.com/Capsule7446/window-scaler/issues/4)跟踪。编译产物、日志及本地设置由 `.gitignore` 排除，`Cargo.lock` 随源码提交。
