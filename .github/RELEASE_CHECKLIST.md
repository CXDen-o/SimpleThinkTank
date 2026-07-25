# 发布检查清单（每次交付前必做）

> 用法：每次发布前逐项确认。带 `[ ]` 的项全部变为 `[x]` 后才允许打 tag。

## 1. 敏感数据检查

- [ ] 运行 `npm run preflight` 全部通过（自动扫描：API Key / Token、个人邮箱、个人本地路径、内网 IP、误入库文件、版本一致性）
- [ ] 人工复核本次新增依赖的许可证：MIT / Apache-2.0 / BSD 可用；GPL / SSPL / 商用受限不可用
- [ ] 人工复核新增代码与配置：无公司内部信息（域名、项目编号、内部工具水印）、无他人版权代码片段
- [ ] 新增密钥类配置一律走环境变量 / 系统设置页，禁止硬编码

## 2. 仓库卫生

- [ ] `git status` 无意外文件；确认 `docs/`、`*.db`、`.env`、`dl.ps1`、`download_ollama.py`、`tags.json`、`src-tauri/PROGRESS.md` 未被跟踪
- [ ] README.md 内容仍然准确（功能列表、安装步骤、版本号相关描述）
- [ ] CHANGELOG.md 已添加本次版本的条目

## 3. 质量门禁

- [ ] `npm run build`（含 vue-tsc 类型检查）通过
- [ ] `npm run lint` 无新增错误
- [ ] 手动冒烟测试：导入文档 → 切分 → 向量化 → 流式问答 → 重启后数据仍在

## 4. 版本号（三处必须一致，`npm run preflight` 会校验）

- [ ] `package.json` → `version`
- [ ] `src-tauri/Cargo.toml` → `version`
- [ ] `src-tauri/tauri.conf.json` → `version`

## 5. 发布

- [ ] 提交全部改动：`git add -A && git commit -m "chore(release): vX.Y.Z"`
- [ ] 打 tag 并推送：`git tag vX.Y.Z && git push origin main --tags`
- [ ] 等待 GitHub Actions 生成草稿 Release，核对：
  - 安装包 `*_x64-setup.exe` 可下载
  - `SHA256SUMS.txt` 已附带
- [ ] 从 CHANGELOG 摘录本次内容填入 Release 说明，确认无误后发布
