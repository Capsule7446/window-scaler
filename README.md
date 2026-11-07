# 窗口缩放器

Windows独立工具，使用Rust + GPUI Kit。直接修改目标程序的原窗口尺寸。

## 使用

1. 双击 `WindowScaler.exe`，搜索并勾选需要调整的窗口。
2. 输入目标内容区宽度（默认240像素），点击“缩小”。“放大”执行系统最大化。
3. 关闭主界面后，左键单击托盘图标可重新打开；右键选择退出，会先放大本次缩过的窗口。

过滤后仍保留勾选：按钮会操作全部已选窗口，包括被搜索条件隐藏的窗口。界面显示隐藏的已选数量。

仅保存宽度，配置位于 `%LOCALAPPDATA%\WindowScaler\settings.json`。工具重开后搜索和勾选清空。

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
.\build.ps1 -Mode release
```

产物为 `dist\WindowScaler.exe`。依赖通过 `Cargo.lock` 固定。

## 项目与任务

- 代码仓库：[Capsule7446/window-scaler](https://github.com/Capsule7446/window-scaler)
- Ticket跟踪：[GitHub Issues](https://github.com/Capsule7446/window-scaler/issues)
- [已确认规格](docs/spec.md) · [验证记录](docs/validation.md) · [任务跟踪规则](docs/agents/issue-tracker.md)
- [分支与发布流程](docs/branching.md)：`feature → epic → release → master`；`hotfix`负责修复。当前大功能为 [Epic #5](https://github.com/Capsule7446/window-scaler/issues/5)。

`release/vX.Y.Z`会自动测试打包，合入master后由GitHub Actions自动打tag并发布ZIP、EXE和校验文件；正式包见 [GitHub Releases](https://github.com/Capsule7446/window-scaler/releases)。

目前已通过8项测试和Release构建；[托盘完整流程 #3](https://github.com/Capsule7446/window-scaler/issues/3)与[正式版及实机兼容验收 #4](https://github.com/Capsule7446/window-scaler/issues/4)保持打开。编译产物、日志及本地设置由 `.gitignore` 排除，`Cargo.lock` 随源码提交。
