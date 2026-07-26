# 智识库 SimpleThinkTank

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![GitHub release](https://img.shields.io/github/v/release/CXDen-o/SimpleThinkTank)](https://github.com/CXDen-o/SimpleThinkTank/releases)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11-blue)](https://github.com/CXDen-o/SimpleThinkTank/releases)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-24C8D8)](https://tauri.app)

本地私有化 RAG 知识库桌面应用：基于 Ollama 本地大模型与 SQLite 向量检索，文档与对话数据完全保存在本机，不经过任何云端服务。

## 功能特性

- **多知识库管理**：创建 / 重命名 / 删除，知识库运行时热切换
- **文档导入**：PDF / DOCX / TXT / MD，支持文件夹批量导入与拖拽导入，实时进度展示
- **切分策略**：固定长度 / 语义 / Agentic 智能切分，参数可调、切分结果可预览
- **向量检索**：sqlite-vec 静态链接，单文件数据库，毫秒级 KNN 检索
- **流式问答**：检索增强生成，逐 token 流式输出，引用来源可溯源
- **智能滚动**：流式输出贴底跟随，上翻自由阅读，一键回到最新
- **Ollama 托管**：自动检测 / 启动 / 退出确认，未安装时提供静默安装引导
- **存储统计**：知识库文档 / 片段 / 向量 / 磁盘占用一目了然
- **完全本地**：数据存于 `文档/SimpleThinkTank/` 目录，卸载即清

## 下载安装

从 [Releases](https://github.com/CXDen-o/SimpleThinkTank/releases) 下载最新的 `*_x64-setup.exe`（Windows 10/11 64 位）：

1. 双击运行安装包
2. 首次启动自动检测 Ollama；未安装时按引导一键静默安装
3. 应用自动拉取默认模型（对话 `qwen3:1.7b` + 嵌入 `nomic-embed-text`），完成后即可使用

> 安装包未做代码签名，Windows SmartScreen 可能提示"未知发布者"，选择"仍要运行"即可。
> 可使用 Release 页附带的 `SHA256SUMS.txt` 校验安装包完整性：
> `certutil -hashfile SimpleThinkTank_0.1.0_x64-setup.exe SHA256`

## 截图

（待补充：主界面 / 问答界面截图，存放于 `.github/assets/`）

## 技术栈

| 层 | 技术 |
|---|---|
| 桌面壳 | Tauri 2（Rust） |
| 前端 | Vue 3 + TypeScript + Element Plus + Pinia |
| 数据库 | SQLite（sqlx）+ sqlite-vec 向量扩展 |
| 模型运行时 | Ollama（qwen3:1.7b / nomic-embed-text） |
| 文档解析 | pdf-extract / docx-rs |

## 本地开发

前置要求：Node.js 20+、Rust（stable）、MSVC Build Tools（Windows）、本机已安装 Ollama。

```bash
npm install
npm run tauri dev
```

## 构建安装包

```bash
npm run tauri build
```

产物位于 `src-tauri/target/release/bundle/nsis/`。

## 发布流程

打 tag 触发 GitHub Actions 自动构建并生成**草稿 Release**（含安装包与 SHA256 校验文件）：

```bash
git tag v0.1.0
git push origin v0.1.0
```

每次发布前请完成 [.github/RELEASE_CHECKLIST.md](.github/RELEASE_CHECKLIST.md) 中的检查（敏感数据扫描 `npm run preflight`、版本一致性、更新日志等）。

## 项目结构

```
src/            Vue 前端（views / stores / api / composables）
src-tauri/      Rust 后端（ollama / rag / chunking / vectorstore / parsing / commands）
scripts/        开发辅助脚本（发布前检查等）
```

## 贡献

欢迎 Issue 与 Pull Request。提交前请运行：

```bash
npm run preflight   # 敏感数据扫描 + 版本一致性检查
npm run build       # 类型检查 + 前端构建
```

## License

[MIT](LICENSE)
