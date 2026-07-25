<template>
  <div class="settings-window">
    <header class="settings-header">
      <span class="settings-title">参数设置</span>
      <el-tag v-if="allModelsInstalled" type="success" size="small">默认模型已就绪</el-tag>
      <el-tag v-else type="warning" size="small">默认模型未就绪</el-tag>
    </header>

    <main class="settings-body">
      <el-tabs v-model="settingsTab">
        <el-tab-pane label="问答参数" name="qa">
          <el-form v-if="settingsLoaded" label-width="110px" size="default">
            <el-form-item label="Top K">
              <el-input-number
                v-model="localQueryOptions.top_k"
                :min="1"
                :max="20"
                :step="1"
                controls-position="right"
              />
            </el-form-item>
            <el-form-item label="Temperature">
              <el-slider
                v-model="localQueryOptions.temperature"
                :min="0"
                :max="2"
                :step="0.1"
                style="width: 320px"
              />
            </el-form-item>
            <el-form-item label="Max Tokens">
              <el-input-number
                v-model="localQueryOptions.max_tokens"
                :min="128"
                :max="8192"
                :step="128"
                controls-position="right"
              />
            </el-form-item>
            <el-form-item label="使用历史">
              <el-switch v-model="localQueryOptions.use_history" />
            </el-form-item>
            <el-form-item>
              <el-button type="primary" @click="applyQueryOptions">应用到当前对话</el-button>
            </el-form-item>
          </el-form>
          <el-skeleton v-else :rows="4" animated />
        </el-tab-pane>

        <el-tab-pane label="模型下载源" name="download">
          <el-alert
            v-if="!allModelsInstalled"
            title="默认模型尚未完全下载"
            type="info"
            :closable="false"
            show-icon
            style="margin-bottom: 16px"
          >
            <template #default>
              建议先配置镜像源或代理以加速下载,再点击"下载默认模型"。
            </template>
          </el-alert>

          <el-form label-width="140px" size="default">
            <el-form-item label="Ollama 服务地址">
              <el-input
                v-model="systemStore.settings.ollama_base_url"
                placeholder="留空使用默认 http://127.0.0.1:11434"
              />
            </el-form-item>
            <el-form-item label="镜像源">
              <el-input
                v-model="systemStore.settings.ollama_registry"
                placeholder="如 https://hf-mirror.com/ollama(留空用官方)"
              />
            </el-form-item>
            <el-form-item label="HTTPS 代理">
              <el-input
                v-model="systemStore.settings.https_proxy"
                placeholder="如 http://127.0.0.1:7890(留空不启用)"
              />
            </el-form-item>
            <el-form-item label="自定义模型目录">
              <el-switch v-model="systemStore.settings.use_custom_models_dir" />
              <span class="hint-text">
                开启后模型存放于 ~/Documents/Zhishiku/models/
              </span>
            </el-form-item>
            <el-form-item label="下载重试次数">
              <el-input-number
                v-model="systemStore.settings.download_max_retries"
                :min="1"
                :max="10"
                controls-position="right"
              />
            </el-form-item>
            <el-form-item label="连接超时(秒)">
              <el-input-number
                v-model="systemStore.settings.download_connect_timeout_secs"
                :min="5"
                :max="300"
                controls-position="right"
              />
            </el-form-item>
            <el-form-item label="请求超时(秒)">
              <el-input-number
                v-model="systemStore.settings.download_request_timeout_secs"
                :min="60"
                :max="3600"
                :step="60"
                controls-position="right"
              />
            </el-form-item>
          </el-form>

          <div class="settings-actions">
            <el-button
              :icon="Connection"
              :loading="systemStore.testingSource"
              @click="systemStore.testSource()"
            >
              测试连接
            </el-button>
            <el-button
              type="primary"
              :icon="DocumentCopy"
              :loading="systemStore.savingSettings"
              @click="onSaveSettings"
            >
              保存设置
            </el-button>
            <el-tag
              v-if="systemStore.lastTestResult"
              :type="systemStore.lastTestResult.ok ? 'success' : 'danger'"
              size="default"
            >
              {{ systemStore.lastTestResult.ok ? "成功" : "失败" }} -
              {{ systemStore.lastTestResult.latency_ms }}ms
              ({{ systemStore.lastTestResult.message }})
            </el-tag>
          </div>

          <!-- 下载进度区 -->
          <el-divider content-position="left">
            <span>模型下载进度</span>
          </el-divider>

          <!-- 模型已安装时的展示 -->
          <div v-if="allModelsInstalled && !systemStore.downloadingModels" class="installed-summary">
            <el-icon color="var(--el-color-success)" :size="20">
              <CircleCheckFilled />
            </el-icon>
            <span>所有默认模型已安装,无需下载。</span>
          </div>

          <!-- 未安装且有进度或正在下载 -->
          <div v-else-if="systemStore.downloadingModels || hasProgress" class="progress-list">
            <div
              v-for="model in progressModels"
              :key="model"
              class="progress-item"
            >
              <div class="progress-header">
                <span class="model-name">{{ model }}</span>
                <el-tag :type="statusTagType(progressMap[model].status)" size="small">
                  {{ statusLabel(progressMap[model].status) }}
                </el-tag>
                <template v-if="!isInstalled(progressMap[model])">
                  <span class="attempt" v-if="progressMap[model].max_attempts > 1">
                    第 {{ progressMap[model].attempt }} / {{ progressMap[model].max_attempts }} 次
                  </span>
                  <el-button
                    v-if="canCancel(progressMap[model])"
                    size="small"
                    type="danger"
                    plain
                    @click="systemStore.cancelDownload(model)"
                  >
                    取消
                  </el-button>
                </template>
              </div>
              <template v-if="!isInstalled(progressMap[model])">
                <el-progress
                  :percentage="progressPercent(progressMap[model])"
                  :format="progressFormat"
                  :status="progressStatus(progressMap[model].status)"
                  :stroke-width="16"
                  :text-inside="true"
                />
                <div class="progress-meta">
                  <span>
                    {{ formatBytes(progressMap[model].completed) }} /
                    {{ formatBytes(progressMap[model].total) }}
                  </span>
                  <span>速率: {{ formatRate(progressMap[model].rate_bps) }}</span>
                  <span>ETA: {{ formatEta(progressMap[model].eta_secs) }}</span>
                  <span v-if="progressMap[model].error" class="error-text">
                    {{ progressMap[model].error }}
                  </span>
                </div>
              </template>
            </div>
            <div v-if="hasFailedOrCancelled" class="retry-row">
              <el-button
                type="warning"
                :icon="Refresh"
                @click="systemStore.retryDownload()"
              >
                重试下载
              </el-button>
            </div>
          </div>

          <!-- 未安装且无进度:显示下载按钮 -->
          <div v-else class="no-progress">
            <el-button
              type="primary"
              :icon="Download"
              @click="systemStore.downloadDefaultModels()"
            >
              下载默认模型 (qwen3:1.7b + nomic-embed-text)
            </el-button>
            <span class="hint-text" style="margin-left: 12px">
              下载约需 1.5GB 磁盘空间,建议先配置镜像源/代理
            </span>
          </div>
        </el-tab-pane>

        <el-tab-pane label="存储统计" name="storage">
          <div class="settings-actions" style="margin-top: 0">
            <el-button
              :icon="Refresh"
              :loading="systemStore.storageStatsLoading"
              @click="systemStore.fetchStorageStats()"
            >
              刷新
            </el-button>
            <el-button
              type="danger"
              plain
              :loading="systemStore.clearingLogs"
              @click="handleClearLogs"
            >
              清空日志
            </el-button>
          </div>

          <el-skeleton v-if="systemStore.storageStatsLoading && !storageStats" :rows="4" animated />
          <template v-else-if="storageStats">
            <div class="storage-overview">
              <div class="stat-item">
                <div class="stat-value">{{ formatBytes(storageStats.total_bytes) }}</div>
                <div class="stat-label">应用数据合计</div>
              </div>
              <div class="stat-item">
                <div class="stat-value">{{ formatBytes(storageStats.database_bytes) }}</div>
                <div class="stat-label">数据库(含向量)</div>
              </div>
              <div class="stat-item">
                <div class="stat-value">{{ formatBytes(storageStats.knowledge_bases_bytes) }}</div>
                <div class="stat-label">知识库文件</div>
              </div>
              <div class="stat-item">
                <div class="stat-value">{{ formatBytes(storageStats.logs_bytes) }}</div>
                <div class="stat-label">日志</div>
              </div>
            </div>

            <el-divider content-position="left">知识库明细</el-divider>
            <el-table
              v-if="storageStats.knowledge_bases.length > 0"
              :data="storageStats.knowledge_bases"
              size="small"
              style="width: 100%"
            >
              <el-table-column prop="name" label="知识库" min-width="110" show-overflow-tooltip />
              <el-table-column prop="document_count" label="文档" width="64" align="right" />
              <el-table-column prop="chunk_count" label="分块" width="76" align="right" />
              <el-table-column prop="vector_count" label="向量" width="76" align="right" />
              <el-table-column label="原文件" width="88" align="right">
                <template #default="{ row }">{{ formatBytes(row.documents_bytes) }}</template>
              </el-table-column>
              <el-table-column label="磁盘占用" width="96" align="right">
                <template #default="{ row }">{{ formatBytes(row.dir_bytes) }}</template>
              </el-table-column>
            </el-table>
            <div v-else class="no-progress">暂无知识库</div>

            <el-divider content-position="left">模型目录</el-divider>
            <div
              v-for="dir in storageStats.model_dirs"
              :key="dir.path"
              class="model-dir-item"
            >
              <el-tag v-if="dir.exists" type="success" size="small">存在</el-tag>
              <el-tag v-else type="info" size="small">未创建</el-tag>
              <span class="model-dir-path" :title="dir.path">{{ dir.path }}</span>
              <span class="model-dir-size">{{ dir.exists ? formatBytes(dir.bytes) : "-" }}</span>
            </div>
          </template>
        </el-tab-pane>
      </el-tabs>
    </main>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, watch } from "vue";
import {
  Download,
  Refresh,
  Connection,
  DocumentCopy,
  CircleCheckFilled,
} from "@element-plus/icons-vue";
import { ElMessage, ElMessageBox } from "element-plus";
import { emit } from "@tauri-apps/api/event";
import { useSystemStore, formatRate, formatEta } from "@/stores/system";
import { useChatStore } from "@/stores/chat";
import { api, events, type QueryOptions } from "@/api/invoke";

const systemStore = useSystemStore();
const chatStore = useChatStore();

const settingsTab = ref<"qa" | "download" | "storage">("qa");

/** 存储统计数据(store 中可能为 null,初次进入 tab 时拉取) */
const storageStats = computed(() => systemStore.storageStats);

// 首次切到"存储统计"tab 时拉取数据
watch(settingsTab, (tab) => {
  if (tab === "storage" && !systemStore.storageStats) {
    systemStore.fetchStorageStats();
  }
});

/** 清空日志(带确认) */
async function handleClearLogs() {
  try {
    await ElMessageBox.confirm(
      "将删除日志目录中的历史日志文件,不影响知识库数据。是否继续?",
      "清空日志",
      { type: "warning", confirmButtonText: "清空", cancelButtonText: "取消" }
    );
  } catch {
    return; // 用户取消
  }
  await systemStore.clearLogs();
}

/** 设置是否加载完成(避免问答参数 UI 闪烁默认值后被覆盖) */
const settingsLoaded = ref(false);

/** 本地查询参数副本(独立窗口中编辑,点"应用"才同步到主窗口) */
const localQueryOptions = ref<QueryOptions>({ ...chatStore.queryOptions });

// ===== 下载进度 UI 辅助 =====
interface ProgressLike {
  status: string;
  completed: number | null;
  total: number | null;
  percent: number | null;
  rate_bps: number | null;
  eta_secs: number | null;
  attempt: number;
  max_attempts: number;
  error: string | null;
}

const progressMap = computed(() => systemStore.progressMap as Record<string, ProgressLike>);
const progressModels = computed(() => Object.keys(progressMap.value));
const hasProgress = computed(() => progressModels.value.length > 0);

const hasFailedOrCancelled = computed(() =>
  Object.values(progressMap.value).some(
    (p) => p.status === "error" || p.status === "cancelled"
  )
);

const allModelsInstalled = computed(
  () => systemStore.modelsOnDisk?.all_installed === true
);

function progressPercent(p: ProgressLike): number {
  if (p.percent !== null && p.percent !== undefined) {
    return Math.min(100, Math.max(0, p.percent * 100));
  }
  if (p.completed !== null && p.total && p.total > 0) {
    return Math.min(100, Math.max(0, (p.completed / p.total) * 100));
  }
  return 0;
}

function progressFormat(percentage: number): string {
  return `${percentage.toFixed(1)}%`;
}

function canCancel(p: ProgressLike): boolean {
  return p.status === "pulling" || p.status === "retrying";
}

function isInstalled(p: ProgressLike): boolean {
  return p.status === "installed";
}

function statusTagType(status: string): "primary" | "success" | "warning" | "danger" | "info" {
  switch (status) {
    case "success":
    case "installed":
      return "success";
    case "error":
      return "danger";
    case "cancelled":
    case "retrying":
      return "warning";
    case "pulling":
      return "primary";
    default:
      return "info";
  }
}

function statusLabel(status: string): string {
  const map: Record<string, string> = {
    pulling: "下载中",
    success: "完成",
    error: "失败",
    cancelled: "已取消",
    retrying: "重试中",
    installed: "已安装",
  };
  return map[status] || status;
}

function progressStatus(status: string): "" | "success" | "exception" | "warning" {
  if (status === "success" || status === "installed") return "success";
  if (status === "error") return "exception";
  if (status === "cancelled" || status === "retrying") return "warning";
  return "";
}

function formatBytes(n: number | null): string {
  if (n === null || n === undefined) return "-";
  if (n < 1024) return `${n} B`;
  const units = ["B", "KB", "MB", "GB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(1)} ${units[i]}`;
}

/** 将本地问答参数应用到主窗口的 chatStore 并持久化到后端 */
async function applyQueryOptions() {
  chatStore.queryOptions = { ...localQueryOptions.value };
  await emit("query-options-changed", chatStore.queryOptions);
  // 持久化到后端 settings.query_options
  try {
    systemStore.settings.query_options = JSON.stringify(localQueryOptions.value);
    await api.updateAppSettings(systemStore.settings);
  } catch (e) {
    // 持久化失败不影响当前会话应用
    console.error("持久化问答参数失败", e);
  }
  ElMessage.success("已应用到当前对话并保存");
}

/** 保存设置后刷新本地模型检查 */
async function onSaveSettings() {
  await systemStore.saveSettings();
  await systemStore.checkModelsOnDisk();
}

// 并行加载所有设置，减少白屏时间
async function loadSettingsParallel() {
  const [, ,] = await Promise.all([
    systemStore.loadSettings().catch(() => {}),
    systemStore.checkModelsOnDisk().catch(() => {}),
    systemStore.checkSystem().catch(() => {}),
  ]);
}

onMounted(async () => {
  // 先快速加载本地缓存的查询参数
  try {
    const cached = localStorage.getItem("zhishiku_query_options");
    if (cached) {
      const opts = JSON.parse(cached);
      localQueryOptions.value = { ...localQueryOptions.value, ...opts };
    }
  } catch {
    // ignore
  }
  
  // 并行加载所有设置
  await loadSettingsParallel();
  
  // 从持久化的 settings.query_options 初始化本地问答参数
  try {
    if (systemStore.settings.query_options) {
      const opts = JSON.parse(systemStore.settings.query_options);
      localQueryOptions.value = { ...localQueryOptions.value, ...opts };
      // 缓存到 localStorage
      localStorage.setItem("zhishiku_query_options", systemStore.settings.query_options);
    }
  } catch {
    // 解析失败保持默认
  }
  
  // 设置加载完成,渲染问答参数表单(避免闪烁)
  settingsLoaded.value = true;
  
  // 监听主窗口发来的问答参数同步
  await events.onQueryOptionsChanged((opts) => {
    localQueryOptions.value = { ...opts };
  });
});
</script>

<style scoped>
.settings-window {
  display: flex;
  flex-direction: column;
  height: 100vh;
  background: var(--el-bg-color-page);
}

.settings-header {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 16px 28px;
  background: var(--el-bg-color);
  border-bottom: 1px solid var(--el-border-color-light);
  flex-shrink: 0;
}

.settings-title {
  font-size: 15px;
  font-weight: 600;
  flex: 1;
  color: var(--el-text-color-primary);
}

.settings-body {
  flex: 1;
  overflow-y: auto;
  padding: 24px 28px;
}

.settings-body :deep(.el-form-item) {
  margin-bottom: 18px;
}

.hint-text {
  font-size: 11px;
  color: var(--el-text-color-secondary);
  margin-left: 10px;
}

.settings-actions {
  display: flex;
  align-items: center;
  gap: 12px;
  margin: 18px 0;
  flex-wrap: wrap;
}

.installed-summary {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 16px;
  background: var(--el-color-success-light-9);
  border: 1px solid var(--el-color-success-light-7);
  border-radius: 4px;
  color: var(--el-color-success-dark-2);
  font-size: 13px;
}

.no-progress {
  display: flex;
  align-items: center;
  padding: 10px 0;
}

.progress-list {
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 10px 0;
}

.storage-overview {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 12px;
  margin-bottom: 8px;
}

.stat-item {
  padding: 14px 16px;
  background: var(--el-fill-color-light);
  border: 1px solid var(--el-border-color-lighter);
  border-radius: 6px;
}

.stat-value {
  font-size: 18px;
  font-weight: 600;
  color: var(--el-text-color-primary);
  font-variant-numeric: tabular-nums;
}

.stat-label {
  margin-top: 4px;
  font-size: 12px;
  color: var(--el-text-color-secondary);
}

.model-dir-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 0;
  font-size: 12px;
}

.model-dir-path {
  flex: 1;
  color: var(--el-text-color-regular);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.model-dir-size {
  color: var(--el-text-color-primary);
  font-variant-numeric: tabular-nums;
  flex-shrink: 0;
}

.progress-item {
  border: 1px solid var(--el-border-color-light);
  border-radius: 4px;
  padding: 14px;
  background: var(--el-bg-color);
}

.progress-header {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 10px;
}

.model-name {
  font-weight: 600;
  font-size: 13px;
  color: var(--el-text-color-primary);
}

.attempt {
  font-size: 11px;
  color: var(--el-text-color-secondary);
}

.progress-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 14px;
  margin-top: 8px;
  font-size: 11px;
  color: var(--el-text-color-secondary);
}

.error-text {
  color: var(--el-color-danger);
}

.retry-row {
  margin-top: 6px;
}
</style>
