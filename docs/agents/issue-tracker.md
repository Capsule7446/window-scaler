# GitHub Issues

任务仓库：`Capsule7446/window-scaler`。本文件的跟踪规则适用于本仓库，取代父工作区曾使用的 `.scratch/window-scaler/issues/` 本地任务规则。

## 工作流程

1. 开始任务前读取关联 Issue；缺少编号时先检索已有任务，避免重复创建。
2. 每个可独立验收的任务建立一个 Issue，描述目标、范围、验收清单和依赖；需求定义链接到仓库内的 `docs/spec.md`。
3. 实施进展、阻塞和验证证据更新到该 Issue。仓库文档保存长期规格与说明，Issue 保存任务进度。
4. 提交使用 `Refs #N` 关联任务。验收清单全部完成后，附验证结果并关闭 Issue；未完成的手工验收继续保持打开。

GitHub 不可用时可保留临时草稿，恢复后同步并以 GitHub 状态为准。原本地任务文件仅保存迁移链接，不继续维护第二份活动任务清单。

## 分诊标签

| 标签 | 用途 |
| --- | --- |
| `needs-triage` | 尚待评估的新任务 |
| `needs-info` | 等待补充必要信息 |
| `ready-for-agent` | 范围与验收明确，可由代理实施 |
| `ready-for-human` | 需要人工完成的任务 |
| `wontfix` | 明确不实施的任务 |

每个任务使用一个当前分诊标签。标签表示分诊结果；任务是否完成使用 GitHub 的 open/closed 状态表示。

## 类型与父子任务

- 大功能使用 `epic` 标签；小功能使用 `feature`，并通过GitHub原生sub-issue关系归属于一个Epic。
- 修复使用 `hotfix`；打包、正式发布和版本验收使用 `release`。类型标签与分诊标签分别维护。
- Epic的多个Feature分别建立开发分支，完成后合回对应Epic；具体分支名和合并方向见 [分支与发布流程](../branching.md)。
- 当前大功能为 [Epic #5](https://github.com/Capsule7446/window-scaler/issues/5)，版本发布验收为 [Release #4](https://github.com/Capsule7446/window-scaler/issues/4)。

使用API建立父子关系时，`sub_issue_id`传Issue数据库ID，路径中的父任务使用Issue编号；修改后读取sub-issues列表确认关联。

## CLI 示例

```powershell
gh issue list --repo Capsule7446/window-scaler --state open
gh issue view 1 --repo Capsule7446/window-scaler
gh issue create --repo Capsule7446/window-scaler --title '任务标题' --body-file task.md --label ready-for-agent
```

多行正文使用 UTF-8 文件配合 `--body-file`，保留真实换行。关闭前核对 Issue 编号与最新验收记录。
