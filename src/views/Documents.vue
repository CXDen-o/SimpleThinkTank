<template>
  <div class="doc-layout">
    <!-- 顶栏 -->
    <header class="doc-header">
      <div class="left">
        <el-button :icon="ArrowLeft" text @click="router.back()">返回</el-button>
        <el-divider direction="vertical" />
        <span class="kb-title">{{ kbStore.currentKb?.name || "知识库" }}</span>
      </div>
      <div class="right">
        <el-button :icon="ChatDotRound" @click="router.push(`/kb/${kbId}/chat`)">
          开始问答
        </el-button>
        <el-button :icon="FolderOpened" @click="triggerUploadFolder">
          导入文件夹
        </el-button>
        <el-button type="primary" :icon="Upload" @click="triggerUpload">
          导入文档
        </el-button>
      </div>
    </header>

    <!-- 导入进度条 -->
    <div v-if="docStore.importing" class="progress-bar">
      <el-progress
        :percentage="Math.round(docStore.displayPercent)"
        :status="docStore.importProgress && docStore.importProgress.failed > 0 ? 'warning' : undefined"
        class="progress-flex"
      />
      <span class="progress-text" v-if="docStore.importProgress">
        正在处理: {{ docStore.importProgress.current_file || '准备中...' }}
        ({{ docStore.importProgress.completed }}/{{ docStore.importProgress.total }})
      </span>
      <span class="progress-text" v-else>
        准备导入...
      </span>
      <el-button size="small" type="danger" text @click="docStore.cancelImport()">
        取消
      </el-button>
    </div>

    <!-- 文档列表 -->
    <main class="doc-main" :class="{ 'drag-over': isDragOver }">
      <div v-if="isDragOver" class="drag-hint">
        <el-icon :size="48"><Upload /></el-icon>
        <p>松开鼠标导入文件</p>
      </div>
      <el-table
        :data="docStore.documents"
        v-loading="docStore.loading"
        style="width: 100%"
        :empty-text="'暂无文档，点击右上角导入'"
        :header-cell-style="{ background: 'var(--el-fill-color-lighter)' }"
        stripe
      >
        <el-table-column type="index" width="56" align="center" />
        <el-table-column prop="file_name" label="文件名" min-width="240" class-name="align-left">
          <template #default="{ row }">
            <el-icon class="file-icon"><Document /></el-icon>
            {{ row.file_name }}
          </template>
        </el-table-column>
        <el-table-column prop="file_type" label="类型" width="80" align="center">
          <template #default="{ row }">
            <el-tag size="small">{{ row.file_type.toUpperCase() }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="大小" width="100" align="center">
          <template #default="{ row }">
            {{ docStore.formatSize(row.file_size) }}
          </template>
        </el-table-column>
        <el-table-column label="状态" width="100" align="center">
          <template #default="{ row }">
            <el-tag :type="docStore.statusType(row.status)" size="small">
              {{ docStore.statusText(row.status) }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column prop="chunk_count" label="分块" width="80" align="center" />
        <el-table-column label="导入时间" width="180" align="center">
          <template #default="{ row }">
            {{ new Date(row.created_at).toLocaleString("zh-CN") }}
          </template>
        </el-table-column>
        <el-table-column label="操作" width="120" fixed="right" align="center">
          <template #default="{ row }">
            <el-tooltip
              v-if="row.error_message"
              :content="row.error_message"
              placement="top"
            >
              <el-button text :icon="WarningFilled" type="danger" />
            </el-tooltip>
            <el-popconfirm
              title="删除该文档及其索引数据?"
              confirm-button-text="删除"
              cancel-button-text="取消"
              @confirm="docStore.deleteDocument(kbId, row.id)"
            >
              <template #reference>
                <el-button text :icon="Delete" type="danger" />
              </template>
            </el-popconfirm>
          </template>
        </el-table-column>
      </el-table>
    </main>
  </div>
</template>

<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { useRouter } from "vue-router";
import {
  ArrowLeft,
  Upload,
  ChatDotRound,
  Document,
  WarningFilled,
  FolderOpened,
  Delete,
} from "@element-plus/icons-vue";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useKbStore } from "@/stores/kb";
import { useDocStore } from "@/stores/doc";

const props = defineProps<{ id: string }>();
const router = useRouter();
const kbStore = useKbStore();
const docStore = useDocStore();

const kbId = props.id;
const isDragOver = ref(false);
let unlistenDragDrop: (() => void) | null = null;

onMounted(async () => {
  // 如果 store 中没有当前 KB,先加载列表
  if (!kbStore.currentKb || kbStore.currentKb.id !== kbId) {
    await kbStore.loadKnowledgeBases();
    const kb = kbStore.knowledgeBases.find((k) => k.id === kbId);
    if (kb) kbStore.setCurrentKb(kb);
  }
  await docStore.loadDocuments(kbId);

  // 监听原生拖拽事件(Tauri 2.0)
  try {
    const webview = getCurrentWebview();
    unlistenDragDrop = await webview.onDragDropEvent((event) => {
      if (event.payload.type === "over") {
        isDragOver.value = true;
      } else if (event.payload.type === "leave") {
        isDragOver.value = false;
      } else if (event.payload.type === "drop") {
        isDragOver.value = false;
        const paths = event.payload.paths;
        if (paths && paths.length > 0) {
          docStore.importDocuments(kbId, paths);
        }
      }
    });
  } catch (e) {
    console.error("注册拖拽事件失败", e);
  }
});

onUnmounted(() => {
  docStore.importProgress = null;
  docStore.importing = false;
  unlistenDragDrop?.();
});

/** 触发文件选择对话框(支持文件和文件夹) */
async function triggerUpload() {
  const selected = await open({
    multiple: true,
    filters: [
      {
        name: "文档",
        extensions: ["pdf", "docx", "txt", "md", "markdown"],
      },
    ],
  });
  if (!selected || selected.length === 0) return;
  const filePaths = Array.isArray(selected) ? selected : [selected];
  await docStore.importDocuments(kbId, filePaths);
}

/** 选择文件夹导入 */
async function triggerUploadFolder() {
  const selected = await open({
    directory: true,
    multiple: false,
  });
  if (!selected || selected.length === 0) return;
  const filePaths = Array.isArray(selected) ? selected : [selected];
  await docStore.importDocuments(kbId, filePaths);
}
</script>

<style scoped>
.doc-layout {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--el-bg-color-page);
}

.doc-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 28px;
  height: 56px;
  background: var(--el-bg-color);
  border-bottom: 1px solid var(--el-border-color-light);
  flex-shrink: 0;
}

.left {
  display: flex;
  align-items: center;
  gap: 10px;
}

.kb-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--el-text-color-primary);
}

.right {
  display: flex;
  gap: 10px;
}

.progress-bar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 28px;
  background: var(--el-color-warning-light-9);
  border-bottom: 1px solid var(--el-border-color-lighter);
}

.progress-flex {
  flex: 1;
}

.progress-bar :deep(.el-progress-bar__outer),
.progress-bar :deep(.el-progress-bar__inner) {
  transition: width 0.4s ease;
}

.progress-text {
  font-size: 12px;
  color: var(--el-text-color-secondary);
  margin-left: 12px;
}

.doc-main {
  flex: 1;
  overflow: auto;
  padding: 20px 28px;
  position: relative;
  transition: background 0.2s;
}

.doc-main.drag-over {
  background: var(--el-color-primary-light-9);
  outline: 2px dashed var(--el-color-primary);
  outline-offset: -8px;
}

.drag-hint {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  text-align: center;
  color: var(--el-color-primary);
  pointer-events: none;
  z-index: 10;
}

.drag-hint p {
  margin-top: 12px;
  font-size: 14px;
}

.doc-main :deep(.el-table) {
  background: var(--el-bg-color);
  border: 1px solid var(--el-border-color-light);
  border-radius: 4px;
}

.doc-main :deep(.el-table__header-wrapper th) {
  font-size: 12px;
  color: var(--el-text-color-secondary);
}

.file-icon {
  vertical-align: middle;
  margin-right: 6px;
  color: var(--el-color-primary);
}
</style>
