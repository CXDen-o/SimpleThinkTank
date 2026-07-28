// 文档 Pinia Store

import { defineStore } from "pinia";
import { ref } from "vue";
import { api, type Document, type ImportProgress } from "@/api/invoke";
import { events } from "@/api/invoke";
import { ElMessage } from "element-plus";

export const useDocStore = defineStore("doc", () => {
  const documents = ref<Document[]>([]);
  const loading = ref(false);
  const importing = ref(false);
  const importProgress = ref<ImportProgress | null>(null);
  /** 当前导入任务 id(用于取消) */
  const currentTaskId = ref<string | null>(null);
  /** 进度条显示百分比(插值动画) */
  const displayPercent = ref(0);
  let animTimer: ReturnType<typeof setInterval> | null = null;
  /** 后端事件给出的真实进度 */
  let realPercent = 0;
  /** 爬行天花板 = 下一里程碑(渐近逼近,永不越过,不假装完成) */
  let ceiling = 0;

  function stopAnimTimer() {
    if (animTimer) {
      clearInterval(animTimer);
      animTimer = null;
    }
  }

  /**
   * 统一动画 tick:
   * - 落后真实进度 → 快速追赶(rate 0.15)
   * - 已追上真实进度但天花板更高 → 缓慢爬行(rate 0.008),渐近逼近永不到达
   *   保证事件静默期(单文件解析/切分/向量化)进度条持续可见移动
   */
  function ensureAnimTimer() {
    if (animTimer) return;
    animTimer = setInterval(() => {
      const cur = displayPercent.value;
      const creeping = cur > realPercent - 1 && ceiling > realPercent;
      const target = creeping ? ceiling : realPercent;
      const rate = creeping ? 0.008 : 0.15;
      const diff = target - cur;
      if (!creeping && Math.abs(diff) < 0.5) {
        displayPercent.value = target;
        stopAnimTimer(); // 静止时停表
      } else {
        displayPercent.value = cur + diff * rate;
      }
    }, 60);
  }

  let unlistenProgress: (() => void) | null = null;

  /** 加载文档列表 */
  async function loadDocuments(kbId: string) {
    loading.value = true;
    try {
      documents.value = await api.getDocuments(kbId);
    } catch (e) {
      ElMessage.error(`加载文档失败: ${e}`);
    } finally {
      loading.value = false;
    }
  }

  /** 导入文档 */
  async function importDocuments(kbId: string, filePaths: string[]) {
    if (filePaths.length === 0) return;
    
    // 重置状态(先重置 importing 以隐藏进度条,再清零进度,最后显示)
    importProgress.value = null;
    displayPercent.value = 0;
    realPercent = 0;
    ceiling = 0;
    stopAnimTimer();
    // 短暂等待 DOM 更新后重新显示进度条
    await new Promise((r) => setTimeout(r, 50));
    importing.value = true;

    // 监听进度
    if (!unlistenProgress) {
      const unlisten = await events.onImportProgress((p) => {
        importProgress.value = p;
        // 更新真实进度与爬行天花板(下一里程碑,cap 99 避免未完工先显示 100)
        realPercent = p.total > 0
          ? Math.min(100, ((p.completed + p.failed) / p.total) * 100)
          : 0;
        ceiling = p.total > 0
          ? Math.min(99, ((p.completed + p.failed + 1) / p.total) * 100)
          : 0;
        ensureAnimTimer();
        if (p.status === "completed" || p.status === "cancelled") {
          // 完成/取消时 realPercent 已到终值,tick 快速收尾;等待动画完成后再更新状态
          setTimeout(() => {
            importing.value = false;
            currentTaskId.value = null;
            if (p.status === "cancelled") {
              ElMessage.warning(
                `导入已取消: 已完成 ${p.completed} 个, 失败 ${p.failed} 个, 剩余未导入`
              );
            } else {
              ElMessage.success(
                `导入完成: 成功 ${p.completed} 个, 失败 ${p.failed} 个`
              );
            }
            // 刷新文档列表
            loadDocuments(kbId);
            if (unlistenProgress) {
              unlistenProgress();
              unlistenProgress = null;
            }
          }, 600);
        }
      });
      unlistenProgress = unlisten;
    }

    try {
      const taskId = await api.importDocuments({
        knowledge_base_id: kbId,
        file_paths: filePaths,
      });
      currentTaskId.value = taskId;
      ElMessage.info(`开始导入 ${filePaths.length} 个文档（任务 ${taskId}）`);
    } catch (e) {
      importing.value = false;
      ElMessage.error(`导入失败: ${e}`);
    }
  }

  /** 取消当前导入任务 */
  async function cancelImport() {
    if (!currentTaskId.value) return;
    try {
      await api.cancelImport(currentTaskId.value);
    } catch (e) {
      ElMessage.error(`取消失败: ${e}`);
    }
  }

  /** 删除文档(连带清理 chunks 和向量) */
  async function deleteDocument(kbId: string, docId: string) {
    try {
      await api.deleteDocument(docId);
      ElMessage.success("文档已删除");
      await loadDocuments(kbId);
    } catch (e) {
      ElMessage.error(`删除失败: ${e}`);
    }
  }

  /** 格式化文件大小 */
  function formatSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  }

  /** 状态标签颜色 */
  function statusType(status: string): string {
    const map: Record<string, string> = {
      pending: "info",
      parsing: "warning",
      parsed: "success",
      chunking: "warning",
      chunked: "success",
      vectorizing: "warning",
      indexed: "success",
      failed: "danger",
    };
    return map[status] || "info";
  }

  /** 状态中文显示 */
  function statusText(status: string): string {
    const map: Record<string, string> = {
      pending: "待处理",
      parsing: "解析中",
      parsed: "已解析",
      chunking: "切分中",
      chunked: "已切分",
      vectorizing: "向量化中",
      indexed: "已入库",
      failed: "失败",
    };
    return map[status] || status;
  }

  return {
    documents,
    loading,
    importing,
    importProgress,
    displayPercent,
    currentTaskId,
    loadDocuments,
    importDocuments,
    cancelImport,
    deleteDocument,
    formatSize,
    statusType,
    statusText,
  };
});
