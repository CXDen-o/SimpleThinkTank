#!/usr/bin/env node
/**
 * 发布前检查(每次交付前运行): npm run preflight
 *
 * 1. 敏感数据扫描: API Key/Token、个人邮箱、个人本地路径、内网 IP、硬编码密码
 * 2. 版本一致性: package.json = src-tauri/Cargo.toml = src-tauri/tauri.conf.json
 * 3. 误入库检查: docs/、*.db、.env、本地一次性脚本不得被 git 跟踪
 *
 * 退出码: 0 全部通过; 1 存在失败项
 */
import { execSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { extname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("..", import.meta.url));

const SCAN_DIRS = [
  "src",
  "src-tauri/src",
  "src-tauri/capabilities",
  "src-tauri/migrations",
  "scripts",
  ".github",
];
const SCAN_FILES = [
  "package.json",
  "index.html",
  "vite.config.ts",
  "tsconfig.json",
  "tsconfig.node.json",
  "src-tauri/Cargo.toml",
  "src-tauri/tauri.conf.json",
  "src-tauri/build.rs",
  "README.md",
  "CHANGELOG.md",
];
const SKIP_DIRS = new Set(["node_modules", "target", "dist", ".git", "gen", "icons"]);
const SCAN_EXTS = new Set([
  ".ts", ".vue", ".rs", ".json", ".toml", ".md", ".js", ".mjs",
  ".yml", ".yaml", ".html", ".css", ".sql", ".ps1", ".py",
]);
const MAX_FILE_SIZE = 1024 * 1024;

// ---------- 扫描规则 ----------
const RULES = [
  {
    name: "API Key/Token",
    re: /(sk-[A-Za-z0-9_-]{16,}|ghp_[A-Za-z0-9]{20,}|gho_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|AKIA[0-9A-Z]{16}|xox[baprs]-[A-Za-z0-9-]{10,})/,
  },
  {
    name: "硬编码密码",
    re: /(password|passwd)\s*[:=]\s*["'][^"'\s]{6,}["']/i,
  },
  {
    name: "个人本地路径",
    re: /(C:\\Users\\(?!runneradmin\b)[A-Za-z0-9_.-]+|D:\\demo|\/Users\/[a-z0-9_.-]+|\/home\/[a-z0-9_.-]+)/i,
  },
  {
    name: "内网 IP",
    re: /\b(10\.\d{1,3}\.\d{1,3}\.\d{1,3}|192\.168\.\d{1,3}\.\d{1,3}|172\.(1[6-9]|2\d|3[01])\.\d{1,3}\.\d{1,3})\b/,
  },
];
const EMAIL_RE = /[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g;
const EMAIL_ALLOW = ["example.com", "w3.org", "localhost", "schema.tauri.app"];
// 文件名误报豁免(如 128x128@2x.png): 以资源扩展名结尾的"邮箱"实为文件名
const EMAIL_FILE_EXTS = /\.(png|jpe?g|gif|svg|webp|ico|icns|css|js|ts)$/i;

// ---------- 文件遍历 ----------
function* walk(dir) {
  for (const entry of readdirSync(dir)) {
    if (SKIP_DIRS.has(entry)) continue;
    const full = join(dir, entry);
    const st = statSync(full);
    if (st.isDirectory()) yield* walk(full);
    else if (SCAN_EXTS.has(extname(entry)) && st.size < MAX_FILE_SIZE) yield full;
  }
}

function collectFiles() {
  const files = [];
  for (const dir of SCAN_DIRS) {
    const abs = join(ROOT, dir);
    if (existsSync(abs)) files.push(...walk(abs));
  }
  for (const f of SCAN_FILES) {
    const abs = join(ROOT, f);
    if (existsSync(abs)) files.push(abs);
  }
  return files;
}

// ---------- 检查 1: 敏感数据 ----------
function checkSensitive() {
  const findings = [];
  for (const file of collectFiles()) {
    const rel = relative(ROOT, file);
    const lines = readFileSync(file, "utf8").split(/\r?\n/);
    lines.forEach((line, i) => {
      for (const rule of RULES) {
        if (rule.re.test(line)) {
          findings.push(`${rel}:${i + 1}  [${rule.name}]  ${line.trim().slice(0, 100)}`);
        }
      }
      for (const m of line.matchAll(EMAIL_RE)) {
        if (EMAIL_FILE_EXTS.test(m[0])) continue;
        if (!EMAIL_ALLOW.some((d) => m[0].includes(d))) {
          findings.push(`${rel}:${i + 1}  [个人邮箱]  ${m[0]}`);
        }
      }
    });
  }
  return findings;
}

// ---------- 检查 2: 版本一致性 ----------
function checkVersionSync() {
  const pkg = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));
  const tauriConf = JSON.parse(
    readFileSync(join(ROOT, "src-tauri/tauri.conf.json"), "utf8")
  );
  const cargo = readFileSync(join(ROOT, "src-tauri/Cargo.toml"), "utf8");
  const cargoVer = cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1];

  const versions = {
    "package.json": pkg.version,
    "tauri.conf.json": tauriConf.version,
    "Cargo.toml": cargoVer,
  };
  const unique = new Set(Object.values(versions));
  return { versions, ok: unique.size === 1 && !unique.has(undefined) };
}

// ---------- 检查 3: 误入库文件 ----------
function checkTrackedFiles() {
  if (!existsSync(join(ROOT, ".git"))) return { skipped: true, bad: [] };
  const tracked = execSync("git ls-files", { cwd: ROOT, encoding: "utf8" })
    .split(/\r?\n/)
    .filter(Boolean);
  const FORBIDDEN = [
    { re: /^docs\//, why: "内部文档目录" },
    { re: /\.(db|db-wal|db-shm|sqlite3?)$/, why: "数据库文件" },
    { re: /(^|\/)\.env/, why: "环境变量文件" },
    { re: /^(dl\.ps1|download_ollama\.py|tags\.json)$/, why: "本地一次性脚本/残留" },
    { re: /PROGRESS\.md$/, why: "内部进度文档" },
  ];
  const bad = [];
  for (const f of tracked) {
    for (const rule of FORBIDDEN) {
      if (rule.re.test(f)) bad.push(`${f}  (${rule.why})`);
    }
  }
  return { skipped: false, bad };
}

// ---------- 汇总 ----------
let failed = false;

console.log("== 检查 1/3: 敏感数据扫描 ==");
const sensitive = checkSensitive();
if (sensitive.length === 0) {
  console.log("  通过: 未发现 API Key / 个人邮箱 / 个人路径 / 内网 IP / 硬编码密码\n");
} else {
  failed = true;
  console.log(`  发现 ${sensitive.length} 处疑似敏感数据:`);
  sensitive.forEach((f) => console.log(`    ${f}`));
  console.log("");
}

console.log("== 检查 2/3: 版本一致性 ==");
const ver = checkVersionSync();
for (const [k, v] of Object.entries(ver.versions)) {
  console.log(`  ${k}: ${v}`);
}
if (ver.ok) {
  console.log("  通过: 三处版本号一致\n");
} else {
  failed = true;
  console.log("  失败: 版本号不一致,请同步三处\n");
}

console.log("== 检查 3/3: 误入库文件 ==");
const tracked = checkTrackedFiles();
if (tracked.skipped) {
  console.log("  跳过: 尚不是 git 仓库\n");
} else if (tracked.bad.length === 0) {
  console.log("  通过: 无 docs/、数据库、.env、本地脚本被跟踪\n");
} else {
  failed = true;
  console.log(`  发现 ${tracked.bad.length} 个不应入库的文件:`);
  tracked.bad.forEach((f) => console.log(`    ${f}`));
  console.log("  处理: git rm --cached <file> 并确认 .gitignore 规则\n");
}

if (failed) {
  console.log("预检未通过,请先处理上述问题再发布。");
  process.exit(1);
} else {
  console.log("预检全部通过,可以发布。");
}
