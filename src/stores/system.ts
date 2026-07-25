// 系统/Ollama 状态 Pinia Store

import { defineStore } from "pinia";
import { ref, computed } from "vue";
import {
  api,
  events,
  type SystemInfo,
  type AppSettings,
  type DownloadProgressPayload,
  type TestSourceRequest,
  type TestSourceResult,
  type ModelsOnDisk,
  type StorageStats,
} from "@/api/invoke";
import { ElMessage, ElNotification } from "element-plus";

/** 默认模型名(与后端 DEFAULT_* 常量保持一致) */
const DEFAULT_MODELS = ["qwen3:1.7b", "nomic-embed-text"];

function defaultSettings(): AppSettings {
  return {
    ollama_base_url: "",
    ollama_registry: "",
    https_proxy: "",
    use_custom_models_dir: false,
    download_max_retries: 3,
    download_connect_timeout_secs: 30,
    download_request_timeout_secs: 600,
    query_options: '{"top_k":4,"temperature":0.7,"max_tokens":1024,"use_history":true}',
  };
}

export const useSystemStore = defineStore("system", () => {
  const systemInfo = ref<SystemInfo>({
    ollama_installed: false,
    ollama_running: false,
    default_models_available: false,
  });
  const loading = ref(false);
  const installing = ref(false);
  const downloadingModels = ref(false);

  /** 老字段保留兼容性,始终等于 progressMap 中最新进度 */
  const downloadProgress = ref<DownloadProgressPayload | null>(null);

  /** 按 model 维度的进度 map */
  const progressMap = ref<Record<string, DownloadProgressPayload>>({});

  /** 当前活跃下载模型列表(progressMap 中存在条目且未终结的) */
  const activeModels = computed(() =>
    Object.values(progressMap.value).filter(
      (p) => p.status === "pulling" || p.status === "retrying"
    ).map((p) => p.model)
  );

  // ===== 设置状态 =====
  const settings = ref<AppSettings>(defaultSettings());
  const settingsLoading = ref(false);
  const savingSettings = ref(false);
  const testingSource = ref(false);
  const lastTestResult = ref<TestSourceResult | null>(null);

  /** 文件系统层模型存在性(每次启动时刷新,不依赖 Ollama 运行) */
  const modelsOnDisk = ref<ModelsOnDisk | null>(null);

  /** 检查系统状态 */
  async function checkSystem() {
    loading.value = true;
    try {
      systemInfo.value = await api.getSystemInfo();
    } catch (e) {
      ElMessage.error(`检查系统状态失败: ${e}`);
    } finally {
      loading.value = false;
    }
  }

  /** 启动 Ollama */
  async function startOllama() {
    try {
      await api.startOllama();
      await checkSystem();
      ElMessage.success("Ollama 服务已启动");
    } catch (e) {
      ElMessage.error(`启动 Ollama 失败: ${e}`);
    }
  }

  /** 安装 Ollama */
  async function installOllama() {
    installing.value = true;
    try {
      // 后端同步等待静默安装完成(winget 或官方安装包),返回时已装完
      const msg = await api.installOllama();
      ElNotification.success({
        title: "Ollama 安装",
        message: msg,
        duration: 10000,
      });
      await checkSystem();
    } catch (e) {
      ElMessage.error(`安装失败: ${e}`);
    } finally {
      installing.value = false;
    }
  }

  /** 下载默认模型 */
  async function downloadDefaultModels() {
    if (downloadingModels.value) return;
    downloadingModels.value = true;
    progressMap.value = {};

    const unlisten = await events.onModelDownloadProgress((p) => {
      progressMap.value = { ...progressMap.value, [p.model]: p };
      downloadProgress.value = p;
      if (p.status === "success") {
        ElMessage.success(`模型 ${p.model} 下载完成`);
      } else if (p.status === "error") {
        ElMessage.error(
          `模型 ${p.model} 下载失败: ${p.error || "未知错误"}`
        );
      } else if (p.status === "cancelled") {
        ElMessage.warning(`模型 ${p.model} 下载已取消`);
      }
    });

    try {
      await api.downloadDefaultModels();
      const allOk = DEFAULT_MODELS.every(
        (m) => progressMap.value[m]?.status === "success"
      );
      if (allOk) {
        ElMessage.success("默认模型全部下载完成");
      }
      await checkSystem();
      await checkModelsOnDisk();
    } catch (e) {
      ElMessage.error(`模型下载失败: ${e}`);
    } finally {
      downloadingModels.value = false;
      unlisten();
    }
  }

  /** 取消指定模型下载 */
  async function cancelDownload(model: string) {
    try {
      const ok = await api.cancelModelDownload(model);
      if (!ok) {
        ElMessage.warning(`未找到模型 ${model} 的下载任务`);
      }
    } catch (e) {
      ElMessage.error(`取消下载失败: ${e}`);
    }
  }

  /** 重试下载(目前等价于重新触发整体下载,后端会跳过已存在模型) */
  async function retryDownload() {
    await downloadDefaultModels();
  }

  // ===== 设置 CRUD =====

  /** 加载设置 */
  async function loadSettings() {
    settingsLoading.value = true;
    try {
      settings.value = await api.getAppSettings();
    } catch (e) {
      ElMessage.error(`加载设置失败: ${e}`);
      settings.value = defaultSettings();
    } finally {
      settingsLoading.value = false;
    }
  }

  /** 保存设置 */
  async function saveSettings() {
    savingSettings.value = true;
    try {
      await api.updateAppSettings(settings.value);
      ElMessage.success("设置已保存,后续 Ollama 启动将应用新配置");
    } catch (e) {
      ElMessage.error(`保存设置失败: ${e}`);
    } finally {
      savingSettings.value = false;
    }
  }

  /** 测试下载源连通性 */
  async function testSource() {
    testingSource.value = true;
    lastTestResult.value = null;
    try {
      const req: TestSourceRequest = {
        registry: settings.value.ollama_registry || null,
        proxy: settings.value.https_proxy || null,
      };
      lastTestResult.value = await api.testDownloadSource(req);
      if (lastTestResult.value.ok) {
        ElMessage.success(
          `连接成功,延迟 ${lastTestResult.value.latency_ms}ms`
        );
      } else {
        ElMessage.warning(
          `连接失败: ${lastTestResult.value.message} (${lastTestResult.value.latency_ms}ms)`
        );
      }
    } catch (e) {
      ElMessage.error(`测试连接失败: ${e}`);
    } finally {
      testingSource.value = false;
    }
  }

  /** 检查文件系统层模型存在性(不依赖 Ollama 运行) */
  async function checkModelsOnDisk() {
    try {
      modelsOnDisk.value = await api.checkModelsOnDisk();
    } catch (e) {
      ElMessage.error(`检查模型目录失败: ${e}`);
    }
  }

  /** 存储统计数据 */
  const storageStats = ref<StorageStats | null>(null);
  const storageStatsLoading = ref(false);
  const clearingLogs = ref(false);

  /** 拉取存储统计 */
  async function fetchStorageStats() {
    storageStatsLoading.value = true;
    try {
      storageStats.value = await api.getStorageStats();
    } catch (e) {
      ElMessage.error(`获取存储统计失败: ${e}`);
    } finally {
      storageStatsLoading.value = false;
    }
  }

  /** 清空日志并刷新统计 */
  async function clearLogs() {
    clearingLogs.value = true;
    try {
      const freed = await api.clearLogs();
      ElMessage.success(`日志已清理,释放 ${formatBytes(freed)}`);
      await fetchStorageStats();
    } catch (e) {
      ElMessage.error(`清理日志失败: ${e}`);
    } finally {
      clearingLogs.value = false;
    }
  }

  /** 一键就绪:确保 Ollama 安装、运行、模型下载 */
  async function ensureReady() {
    await checkSystem();
    if (!systemInfo.value.ollama_installed) {
      await installOllama();
      await checkSystem();
    }
    if (!systemInfo.value.ollama_running) {
      await startOllama();
    }
    if (!systemInfo.value.default_models_available) {
      await downloadDefaultModels();
    }
  }

  return {
    // state
    systemInfo,
    loading,
    installing,
    downloadingModels,
    downloadProgress,
    progressMap,
    activeModels,
    settings,
    settingsLoading,
    savingSettings,
    testingSource,
    lastTestResult,
    modelsOnDisk,
    storageStats,
    storageStatsLoading,
    clearingLogs,
    // actions
    checkSystem,
    startOllama,
    installOllama,
    downloadDefaultModels,
    cancelDownload,
    retryDownload,
    loadSettings,
    saveSettings,
    testSource,
    checkModelsOnDisk,
    fetchStorageStats,
    clearLogs,
    ensureReady,
  };
});

// ===== 格式化工具函数 =====

/** 格式化字节数 */
export function formatBytes(n: number | null): string {
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

/** 格式化字节速率 */
export function formatRate(bps: number | null): string {
  if (!bps || bps <= 0) return "-";
  const units = ["B/s", "KB/s", "MB/s", "GB/s"];
  let v = bps;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(1)} ${units[i]}`;
}

/** 格式化剩余时间(秒) */
export function formatEta(sec: number | null): string {
  if (!sec || sec <= 0) return "-";
  if (sec < 60) return `${sec}s`;
  const m = Math.floor(sec / 60);
  const s = sec % 60;
  if (m < 60) return `${m}m ${s}s`;
  const h = Math.floor(m / 60);
  const mm = m % 60;
  return `${h}h ${mm}m`;
}

/** 格式化百分比 */
export function formatPercent(p: number | null): string {
  if (p === null || p === undefined) return "-";
  return `${(p * 100).toFixed(1)}%`;
}
