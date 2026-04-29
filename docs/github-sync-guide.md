# GitLab + GitHub 双仓库设置指南

PureTerm 使用两个远程仓库：

- **origin (GitLab)** — 私有仓库，保存完整代码 + CLAUDE.md + AGENTS.md + 工具脚本
- **github (GitHub)** — 开源仓库，只包含代码

## 首次设置

### 1. 添加远程仓库

```bash
cd /Users/demon/Code/PureTerm

# 添加 GitLab 私有仓库
git remote add origin <GitLab仓库URL>

# 添加 GitHub 开源仓库
git remote add github <GitHub仓库URL>
```

### 2. 验证

```bash
git remote -v
```

应输出类似：

```
origin    https://gitlab.example.com/username/PureTerm.git (fetch)
origin    https://gitlab.example.com/username/PureTerm.git (push)
github    https://github.com/username/PureTerm.git (fetch)
github    https://github.com/username/PureTerm.git (push)
```

### 3. 推送

```bash
# 推送到 GitLab（完整备份）
git push -u origin main

# 推送到 GitHub（自动排除 CLAUDE.md / AGENTS.md）
./scripts/push-github.sh
```

## 日常使用

```bash
# 正常开发、提交
git add .
git commit -m "feat: 新功能"

# 推送到 GitLab
git push origin main

# 推送到 GitHub
./scripts/push-github.sh
```

## 修改仓库地址

```bash
# 修改 GitLab 地址
git remote set-url origin <新的GitLab仓库URL>

# 修改 GitHub 地址
git remote set-url github <新的GitHub仓库URL>
```

脚本会自动读取 git 配置中的地址，无需额外修改。

## 删除远程仓库

```bash
# 删除 GitLab
git remote remove origin

# 删除 GitHub
git remote remove github
```
