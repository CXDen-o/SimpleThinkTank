<template>
  <div class="kb-layout">
    <!-- 顶栏 -->
    <header class="kb-header">
      <div class="logo">
        <el-icon :size="24"><Collection /></el-icon>
        <span>智识库</span>
      </div>
      <div class="actions">
        <el-button :icon="Setting" text @click="openSettingsWindow">设置</el-button>
        <el-button type="primary" :icon="Plus" @click="openCreate">
          新建知识库
        </el-button>
      </div>
    </header>

    <!-- 主体:知识库卡片列表 -->
    <main class="kb-main" v-loading="kbStore.loading">
      <div v-if="kbStore.knowledgeBases.length === 0 && !kbStore.loading" class="empty-state">
        <el-empty description="还没有知识库,点击右上角创建第一个">
          <el-button type="primary" :icon="Plus" @click="openCreate">
            新建知识库
          </el-button>
        </el-empty>
      </div>

      <div v-else class="kb-grid">
        <el-card
          v-for="kb in kbStore.knowledgeBases"
          :key="kb.id"
          class="kb-card"
          shadow="hover"
          @click="enterKb(kb)"
        >
          <template #header>
            <div class="card-header">
              <span class="kb-name">{{ kb.name }}</span>
              <el-dropdown trigger="click" @click.stop>
                <el-icon class="more-btn" :size="20" @click.stop><MoreFilled /></el-icon>
                <template #dropdown>
                  <el-dropdown-menu>
                    <el-dropdown-item @click="enterKb(kb)">管理文档</el-dropdown-item>
                    <el-dropdown-item @click="chatKb(kb)">开始问答</el-dropdown-item>
                    <el-dropdown-item @click="kbStore.renameKnowledgeBase(kb)">重命名</el-dropdown-item>
                    <el-dropdown-item divided @click="kbStore.deleteKnowledgeBase(kb)">
                      <span style="color: var(--el-color-danger)">删除</span>
                    </el-dropdown-item>
                  </el-dropdown-menu>
                </template>
              </el-dropdown>
            </div>
          </template>
          <p class="kb-desc">{{ kb.description || "暂无描述" }}</p>
          <div class="kb-meta">
            <el-tag size="small">{{ strategyLabel(kb.split_strategy) }}</el-tag>
            <span class="kb-time">{{ formatTime(kb.created_at) }}</span>
          </div>
        </el-card>
      </div>
    </main>

    <!-- 创建对话框 -->
    <el-dialog v-model="showCreate" title="新建知识库" width="560px" @close="resetForm">
      <el-form :model="form" label-width="130px" class="kb-create-form">
        <el-form-item label="名称" required>
          <el-input v-model="form.name" placeholder="请输入知识库名称" maxlength="50" />
        </el-form-item>
        <el-form-item label="描述">
          <el-input
            v-model="form.description"
            type="textarea"
            :rows="2"
            placeholder="可选,知识库用途描述"
            maxlength="200"
          />
        </el-form-item>
        <el-form-item label="切分策略">
          <el-select v-model="form.split_strategy" placeholder="选择切分策略" style="width: 100%">
            <el-option
              v-for="s in strategies"
              :key="s.id"
              :label="s.name"
              :value="s.id"
            >
              <span>{{ s.name }}</span>
              <span class="strategy-desc">{{ s.description }}</span>
            </el-option>
          </el-select>
        </el-form-item>

        <!-- 动态参数表单 -->
        <template v-if="currentStrategySchema">
          <el-form-item
            v-for="param in currentStrategySchema"
            :key="param.key"
            :label="param.label"
          >
            <el-input-number
              v-if="param.type === 'number'"
              v-model="splitParams[param.key]"
              :min="param.min ?? undefined"
              :max="param.max ?? undefined"
              controls-position="right"
            />
            <!-- 模型选择:下拉框,选项来自本机已安装模型 -->
            <el-select
              v-else-if="param.type === 'string' && param.key === 'model'"
              v-model="splitParamsStr[param.key]"
              :loading="modelsLoading"
              placeholder="选择本机已安装模型"
              style="width: 100%"
            >
              <el-option
                v-for="m in localModels"
                :key="m"
                :label="m"
                :value="m"
              />
              <el-option
                v-if="splitParamsStr[param.key] && !localModels.includes(splitParamsStr[param.key])"
                :label="splitParamsStr[param.key] + ' (当前值,本机未安装)'"
                :value="splitParamsStr[param.key]"
              />
            </el-select>
            <el-input
              v-else-if="param.type === 'string'"
              v-model="splitParamsStr[param.key]"
              :type="Array.isArray(param.default) ? 'text' : 'textarea'"
              :rows="Array.isArray(param.default) ? 1 : 4"
              :placeholder="Array.isArray(param.default) ? '多个分隔符用逗号分隔' : ''"
            />
          </el-form-item>
        </template>

        <!-- 预览区 -->
        <el-form-item v-if="form.name.trim()" label=" ">
          <el-button :icon="View" :loading="previewing" @click="previewSplit">
            预览切分效果
          </el-button>
        </el-form-item>
        <div v-if="previewChunks.length" class="preview-area">
          <div class="preview-header">
            共 {{ previewChunks.length }} 个片段,展示前 3 个:
          </div>
          <div
            v-for="(chunk, idx) in previewChunks.slice(0, 3)"
            :key="idx"
            class="preview-chunk"
          >
            <div class="chunk-meta">片段 {{ idx + 1 }} ({{ chunk.text.length }} 字符)</div>
            <div class="chunk-text">{{ chunk.text.slice(0, 200) }}{{ chunk.text.length > 200 ? '...' : '' }}</div>
          </div>
        </div>
      </el-form>
      <template #footer>
        <el-button @click="showCreate = false">取消</el-button>
        <el-button type="primary" @click="handleCreate" :loading="creating">
          创建
        </el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, reactive, computed, watch } from "vue";
import { useRouter } from "vue-router";
import { Plus, Collection, MoreFilled, View, Setting } from "@element-plus/icons-vue";
import { ElMessage } from "element-plus";
import { useKbStore } from "@/stores/kb";
import { api, type KnowledgeBase, type StrategyInfo, type ChunkResult } from "@/api/invoke";
import { openSettingsWindow } from "@/utils/settingsWindow";

const router = useRouter();
const kbStore = useKbStore();

const showCreate = ref(false);
const creating = ref(false);
const strategies = ref<StrategyInfo[]>([]);
const previewing = ref(false);
const previewChunks = ref<ChunkResult[]>([]);
const splitParams = reactive<Record<string, number>>({});
const splitParamsStr = reactive<Record<string, string>>({});
const localModels = ref<string[]>([]);
const modelsLoading = ref(false);

const form = reactive({
  name: "",
  description: "",
  split_strategy: "fixed_size",
  split_config: "",
});

/** 当前选中策略的参数 schema */
const currentStrategySchema = computed(() => {
  const s = strategies.value.find((x) => x.id === form.split_strategy);
  return s?.config_schema || [];
});

/** 用 schema 默认值初始化参数(数字进 splitParams,字符串进 splitParamsStr) */
function initParamsFromSchema(schema: StrategyInfo["config_schema"]) {
  // 清除旧参数
  Object.keys(splitParams).forEach((k) => delete splitParams[k]);
  Object.keys(splitParamsStr).forEach((k) => delete splitParamsStr[k]);
  for (const p of schema) {
    if (p.type === "number") {
      splitParams[p.key] = typeof p.default === "number" ? p.default : Number(p.default) || 0;
    } else if (p.type === "string") {
      const d = p.default;
      // 数组默认值转逗号分隔字符串
      splitParamsStr[p.key] = Array.isArray(d) ? d.join(", ") : String(d ?? "");
    }
  }
}

/** 加载本机已安装模型(供 model 参数下拉选择) */
async function loadLocalModels() {
  modelsLoading.value = true;
  try {
    localModels.value = await api.listLocalModels();
  } catch {
    localModels.value = [];
  } finally {
    modelsLoading.value = false;
  }
}

/** 切换策略时,用 schema 默认值重置参数 */
watch(currentStrategySchema, (schema) => {
  if (!schema) return;
  initParamsFromSchema(schema);
  // 策略含 model 参数时加载本机模型列表
  if (schema.some((p) => p.key === "model")) {
    loadLocalModels();
  }
});

/** 策略中文名 */
function strategyLabel(id: string): string {
  const s = strategies.value.find((x) => x.id === id);
  return s?.name || id;
}

function openCreate() {
  resetForm();
  showCreate.value = true;
}

function resetForm() {
  form.name = "";
  form.description = "";
  form.split_strategy = "fixed_size";
  previewChunks.value = [];
  // 初始化默认参数
  const schema = strategies.value.find((s) => s.id === "fixed_size")?.config_schema || [];
  initParamsFromSchema(schema);
}

/** 合并数字和字符串参数为提交用对象 */
function buildParams(): Record<string, any> {
  const params: Record<string, any> = {};
  for (const k in splitParams) params[k] = splitParams[k];
  for (const k in splitParamsStr) {
    // 仅当 schema 默认值是数组时(如 separators)按逗号切分为数组;
    // 标量字符串(如 model、prompt_template)原样传递,避免被逗号切碎
    const schema = currentStrategySchema.value.find((p) => p.key === k);
    if (schema && Array.isArray(schema.default)) {
      params[k] = splitParamsStr[k]
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean);
    } else {
      params[k] = splitParamsStr[k];
    }
  }
  return params;
}

/** 预览切分 */
async function previewSplit() {
  previewing.value = true;
  try {
    // 用一段示例文本预览
    const sampleText =
      "智识库是一款基于 Tauri 框架的本地私有化 RAG 桌面应用。\n\n" +
      "它集成了 Ollama 本地模型、向量存储及 langchainrust 框架," +
      "实现从文档导入、智能切分到精准问答的完整闭环。\n\n" +
      "产品针对低配电脑进行了深度优化,提供多种可配置的文档切分策略," +
      "确保在保障数据安全与隐私的前提下,提供流畅的本地知识检索与生成体验。";
    const params = buildParams();
    previewChunks.value = await api.previewSplit(sampleText, form.split_strategy, params);
  } catch (e) {
    previewChunks.value = [];
    ElMessage.error(`预览失败: ${e}`);
  } finally {
    previewing.value = false;
  }
}

async function handleCreate() {
  if (!form.name.trim()) {
    ElMessage.warning("请填写知识库名称");
    return;
  }
  creating.value = true;
  try {
    // 构造 split_config JSON
    const config = buildParams();
    await kbStore.createKnowledgeBase({
      name: form.name.trim(),
      description: form.description.trim() || undefined,
      split_strategy: form.split_strategy,
      split_config: JSON.stringify(config),
    });
    showCreate.value = false;
    resetForm();
  } finally {
    creating.value = false;
  }
}

function enterKb(kb: KnowledgeBase) {
  kbStore.setCurrentKb(kb);
  router.push(`/kb/${kb.id}/documents`);
}

function chatKb(kb: KnowledgeBase) {
  kbStore.setCurrentKb(kb);
  router.push(`/kb/${kb.id}/chat`);
}

function formatTime(iso: string): string {
  return new Date(iso).toLocaleString("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  });
}

onMounted(async () => {
  await kbStore.loadKnowledgeBases();
  // 加载切分策略列表
  try {
    strategies.value = await api.getSplitStrategies();
    // 初始化默认参数
    const schema = strategies.value.find((s) => s.id === "fixed_size")?.config_schema || [];
    initParamsFromSchema(schema);
  } catch (e) {
    console.error("加载切分策略失败", e);
  }
});
</script>

<style scoped>
.kb-layout {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--el-bg-color-page);
}

.kb-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 28px;
  height: 56px;
  background: var(--el-bg-color);
  border-bottom: 1px solid var(--el-border-color-light);
  flex-shrink: 0;
}

.logo {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 16px;
  font-weight: 600;
  color: var(--el-color-primary);
  letter-spacing: 0.3px;
}

.kb-main {
  flex: 1;
  overflow-y: auto;
  padding: 28px;
}

.empty-state {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
}

.kb-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
  gap: 18px;
  max-width: 1400px;
  margin: 0 auto;
}

.kb-card {
  cursor: pointer;
  transition: transform 0.15s ease, box-shadow 0.15s ease;
  border-radius: 4px;
}

.kb-card:hover {
  transform: translateY(-2px);
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.08);
}

.card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.kb-name {
  font-weight: 600;
  font-size: 14px;
  color: var(--el-text-color-primary);
}

.more-btn {
  cursor: pointer;
  color: var(--el-text-color-secondary);
  padding: 10px 12px;
  border-radius: 8px;
  font-size: 24px;
  transition: all 0.15s ease;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 40px;
  min-height: 40px;
  border: 1px solid transparent;
}

.more-btn:hover {
  background: var(--el-fill-color-light);
  color: var(--el-text-color-primary);
  border-color: var(--el-border-color-light);
  transform: scale(1.05);
}

.kb-create-form :deep(.el-form-item__label) {
  white-space: nowrap;
}

.kb-desc {
  color: var(--el-text-color-secondary);
  font-size: 12px;
  line-height: 1.6;
  margin-bottom: 14px;
  min-height: 38px;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.kb-meta {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding-top: 10px;
  border-top: 1px solid var(--el-border-color-extra-light);
}

.kb-time {
  font-size: 11px;
  color: var(--el-text-color-placeholder);
}

.strategy-desc {
  font-size: 11px;
  color: var(--el-text-color-secondary);
  margin-left: 8px;
}

.preview-area {
  margin-top: 12px;
  padding: 12px;
  background: var(--el-fill-color-lighter);
  border-radius: 4px;
  border: 1px solid var(--el-border-color-extra-light);
}

.preview-header {
  font-size: 12px;
  color: var(--el-text-color-secondary);
  margin-bottom: 8px;
}

.preview-chunk {
  margin-bottom: 10px;
  padding: 8px 10px;
  background: var(--el-bg-color);
  border-radius: 3px;
  border: 1px solid var(--el-border-color-extra-light);
}

.chunk-meta {
  font-size: 11px;
  color: var(--el-text-color-secondary);
  margin-bottom: 4px;
}

.chunk-text {
  font-size: 12px;
  line-height: 1.5;
  color: var(--el-text-color-regular);
  white-space: pre-wrap;
  word-break: break-word;
}
</style>
