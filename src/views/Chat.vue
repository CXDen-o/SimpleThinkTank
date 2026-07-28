<template>
  <div class="chat-layout">
    <header class="chat-header">
      <el-button :icon="ArrowLeft" text @click="router.back()">返回</el-button>
      <el-divider direction="vertical" />
      <span class="title">
        {{ kbStore.currentKb?.name || "知识库" }} - 智能问答
      </span>
      <div class="header-right">
        <el-tag v-if="systemStore.systemInfo.ollama_running" type="success" size="small">
          Ollama 运行中
        </el-tag>
        <el-tag v-else type="warning" size="small">Ollama 未运行</el-tag>
        <el-button
          v-if="!systemStore.systemInfo.ollama_running"
          size="small"
          :loading="systemStore.loading"
          @click="systemStore.startOllama()"
        >
          启动
        </el-button>
        <el-button :icon="Setting" text @click="openSettingsWindow">参数</el-button>
      </div>
    </header>

    <main class="chat-main">
      <!-- 左侧对话列表 -->
      <aside class="sidebar">
        <el-button
          type="primary"
          :icon="Plus"
          class="new-chat-btn"
          @click="startNewChat"
        >
          新对话
        </el-button>
        <div class="conversation-list">
          <div
            v-for="conv in chatStore.conversations"
            :key="conv.id"
            class="conversation-item"
            :class="{ active: chatStore.currentConversation?.id === conv.id }"
            @click="chatStore.selectConversation(conv)"
          >
            <el-icon><ChatLineRound /></el-icon>
            <span class="conv-title">{{ conv.title }}</span>
            <el-icon
              class="conv-delete"
              @click.stop="chatStore.deleteConversation(conv)"
            >
              <Delete />
            </el-icon>
          </div>
        </div>
      </aside>

      <!-- 右侧对话区 -->
      <section class="conversation">
        <!-- 消息列表 -->
        <div ref="messagesRef" class="messages">
          <el-empty
            v-if="!chatStore.hasMessages"
            description="开始与你的知识库对话"
          >
            <template #image>
              <el-icon :size="60" color="var(--el-color-primary-light-5)">
                <ChatDotRound />
              </el-icon>
            </template>
          </el-empty>

          <div
            v-for="msg in chatStore.messages"
            :key="msg.id"
            class="message"
            :class="msg.role"
          >
            <div class="avatar">
              <el-icon :size="20">
                <User v-if="msg.role === 'user'" />
                <Promotion v-else />
              </el-icon>
            </div>
            <div class="bubble">
              <div v-if="msg.pending && !msg.content" class="typing">
                <el-icon class="loading"><Loading /></el-icon>
                思考中...
              </div>
              <div v-else class="content">{{ msg.content }}</div>
              <!-- 引用来源 -->
              <div v-if="msg.references && msg.references.length" class="refs">
                <el-divider content-position="left">
                  <span class="refs-title">引用来源 ({{ msg.references.length }})</span>
                </el-divider>
                <el-collapse>
                  <el-collapse-item
                    v-for="(ref, idx) in msg.references"
                    :key="ref.chunk_id"
                    :name="idx"
                  >
                    <template #title>
                      <el-tag size="small" type="info">片段 {{ idx + 1 }}</el-tag>
                      <span class="ref-score">相似度: {{ (ref.score * 100).toFixed(1) }}%</span>
                    </template>
                    <div class="ref-content">{{ ref.content }}</div>
                  </el-collapse-item>
                </el-collapse>
              </div>
            </div>
          </div>
        </div>

        <!-- 回到最新按钮:用户上翻离开底部时出现 -->
        <Transition name="fade">
          <el-button
            v-show="!stickToBottom"
            class="jump-latest"
            circle
            size="large"
            :icon="ArrowDown"
            title="回到最新对话"
            @click="jumpToLatest"
          />
        </Transition>

        <!-- 输入区 -->
        <footer class="input-area">
          <el-input
            v-model="question"
            type="textarea"
            :rows="2"
            :autosize="{ minRows: 1, maxRows: 6 }"
            placeholder="输入你的问题,Enter 发送,Shift+Enter 换行"
            resize="none"
            :disabled="chatStore.querying"
            @keydown.enter.exact.prevent="send"
          />
          <el-button
            type="primary"
            :icon="Promotion"
            :loading="chatStore.querying"
            :disabled="!question.trim() || chatStore.querying"
            @click="send"
          >
            发送
          </el-button>
        </footer>
      </section>
    </main>
  </div>
</template>

<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted } from "vue";
import { useRouter } from "vue-router";
import {
  ArrowLeft,
  ArrowDown,
  Setting,
  Plus,
  ChatLineRound,
  Delete,
  User,
  Promotion,
  ChatDotRound,
  Loading,
} from "@element-plus/icons-vue";
import { ElMessage } from "element-plus";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { useKbStore } from "@/stores/kb";
import { useChatStore } from "@/stores/chat";
import { useSystemStore, DEFAULT_EMBEDDING_MODEL } from "@/stores/system";
import { useStickToBottom } from "@/composables/useStickToBottom";
import { openSettingsWindow } from "@/utils/settingsWindow";
import { events, type QueryOptions } from "@/api/invoke";

const props = defineProps<{ id: string }>();
const router = useRouter();
const kbStore = useKbStore();
const chatStore = useChatStore();
const systemStore = useSystemStore();

// kbId 用 ref 并 watch props.id:同一路由参数变化时 Vue Router 复用组件实例,
// onMounted 不会重跑,必须显式响应参数变化重新初始化
const kbId = ref(props.id);
const question = ref("");
const messagesRef = ref<HTMLElement>();
// 贴底跟随:上翻时流式输出不再强制滚动,回底/点按钮恢复(见 docs/solutions/chat-stick-to-bottom.md)
const { stickToBottom, followIfStuck, scrollToBottom, jumpToLatest } =
  useStickToBottom(messagesRef);
const firstLaunchChecked = ref(false);
let unlistenQueryOpts: UnlistenFn | null = null;

/** 初始化 */
async function init() {
  if (!kbStore.currentKb || kbStore.currentKb.id !== kbId.value) {
    await kbStore.loadKnowledgeBases();
    const kb = kbStore.knowledgeBases.find((k) => k.id === kbId.value);
    if (kb) kbStore.setCurrentKb(kb);
  }
  await chatStore.loadConversations(kbId.value);
  await systemStore.checkSystem();
  await systemStore.loadSettings();
  await systemStore.checkModelsOnDisk();
  // 从持久化的 settings.query_options 初始化问答参数
  try {
    if (systemStore.settings.query_options) {
      const opts = JSON.parse(systemStore.settings.query_options);
      chatStore.queryOptions = { ...chatStore.queryOptions, ...opts };
    }
  } catch {
    // 解析失败保持默认
  }

  // 监听设置窗口发来的问答参数变更
  unlistenQueryOpts = await events.onQueryOptionsChanged((opts: QueryOptions) => {
    chatStore.queryOptions = { ...opts };
  });

  // 首次启动:生效模型未就绪时,自动打开设置窗口引导配置
  if (
    !firstLaunchChecked.value &&
    !systemStore.modelsOnDisk?.all_installed
  ) {
    firstLaunchChecked.value = true;
    openSettingsWindow();
    const locals = systemStore.modelsOnDisk?.local_chat_models ?? [];
    const embeddingReady =
      systemStore.modelsOnDisk?.embedding_model_installed === true;
    if (locals.length > 0 && embeddingReady) {
      // 本地已有对话模型且嵌入模型就绪:提示可免下载直接选用
      ElMessage.info(
        `检测到本机已有对话模型(${locals.join("、")}),可在设置窗口「对话模型」页直接选用,无需下载`
      );
    } else if (locals.length > 0) {
      ElMessage.info(
        `检测到本机已有对话模型(${locals.join("、")}),可在设置窗口选用;另需下载嵌入模型 ${DEFAULT_EMBEDDING_MODEL}`
      );
    } else {
      ElMessage.info("检测到模型未下载,请先在设置窗口中配置下载源后点击下载");
    }
  }
}

onMounted(() => {
  init();
});

// 路由参数变化(组件被复用)时重新初始化:清理旧监听,加载新 KB 的对话
watch(
  () => props.id,
  async (newId) => {
    if (newId && newId !== kbId.value) {
      kbId.value = newId;
      unlistenQueryOpts?.();
      unlistenQueryOpts = null;
      await init();
    }
  }
);

onUnmounted(() => {
  unlistenQueryOpts?.();
});

/** 新建对话 */
function startNewChat() {
  chatStore.newConversation(kbId.value);
}

/** 发送 */
async function send() {
  const q = question.value.trim();
  if (!q || chatStore.querying) return;
  question.value = "";
  await chatStore.ask(kbId.value, q);
  await scrollToBottom();
}

watch(
  () => chatStore.messages.length,
  // 条数变化只发生在发送/切换对话,语义都是"看最新",强制回底
  () => scrollToBottom()
);

// 流式输出时,内容变化仅在贴底跟随中才滚动;用户上翻则释放
watch(
  () => {
    const msgs = chatStore.messages;
    const last = msgs[msgs.length - 1];
    return last?.content;
  },
  () => followIfStuck()
);
</script>

<style scoped>
.chat-layout {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--el-bg-color-page);
}

.chat-header {
  display: flex;
  align-items: center;
  padding: 0 28px;
  height: 56px;
  background: var(--el-bg-color);
  border-bottom: 1px solid var(--el-border-color-light);
  flex-shrink: 0;
}

.title {
  font-size: 14px;
  font-weight: 600;
  flex: 1;
  margin-left: 10px;
  color: var(--el-text-color-primary);
}

.header-right {
  display: flex;
  align-items: center;
  gap: 10px;
}

.chat-main {
  flex: 1;
  display: flex;
  overflow: hidden;
}

.sidebar {
  width: 240px;
  background: var(--el-bg-color);
  border-right: 1px solid var(--el-border-color-light);
  display: flex;
  flex-direction: column;
  padding: 14px;
  flex-shrink: 0;
}

.new-chat-btn {
  width: 100%;
  margin-bottom: 14px;
}

.conversation-list {
  flex: 1;
  overflow-y: auto;
}

.conversation-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  border-radius: 4px;
  cursor: pointer;
  margin-bottom: 4px;
  transition: background 0.15s;
}

.conversation-item:hover {
  background: var(--el-fill-color-light);
}

.conversation-item.active {
  background: var(--el-color-primary-light-9);
  color: var(--el-color-primary);
}

.conv-title {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
}

.conv-delete {
  opacity: 0;
  transition: opacity 0.15s;
  color: var(--el-color-danger);
}

.conversation-item:hover .conv-delete {
  opacity: 1;
}

.conversation {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  position: relative;
}

.jump-latest {
  position: absolute;
  left: 50%;
  transform: translateX(-50%);
  bottom: 96px;
  z-index: 10;
  box-shadow: var(--el-box-shadow);
}

.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.2s ease;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}

.messages {
  flex: 1;
  overflow-y: auto;
  padding: 28px;
}

.message {
  display: flex;
  gap: 14px;
  margin-bottom: 22px;
  max-width: 80%;
}

.message.user {
  flex-direction: row-reverse;
  margin-left: auto;
}

.avatar {
  width: 34px;
  height: 34px;
  border-radius: 50%;
  background: var(--el-fill-color-light);
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.message.user .avatar {
  background: var(--el-color-primary-light-7);
  color: var(--el-color-primary);
}

.message.assistant .avatar {
  background: var(--el-color-success-light-9);
  color: var(--el-color-success);
}

.bubble {
  background: var(--el-bg-color);
  border: 1px solid var(--el-border-color-light);
  border-radius: 6px;
  padding: 12px 16px;
  min-width: 60px;
}

.message.user .bubble {
  background: var(--el-color-primary);
  color: white;
  border-color: var(--el-color-primary);
}

.content {
  white-space: pre-wrap;
  word-break: break-word;
  line-height: 1.65;
  font-size: 13px;
}

.typing {
  color: var(--el-text-color-secondary);
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
}

.loading {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}

.refs {
  margin-top: 10px;
}

.refs-title {
  font-size: 11px;
  color: var(--el-text-color-secondary);
}

.ref-score {
  margin-left: 8px;
  font-size: 11px;
  color: var(--el-text-color-secondary);
}

.ref-content {
  font-size: 12px;
  color: var(--el-text-color-regular);
  background: var(--el-fill-color-lighter);
  padding: 10px;
  border-radius: 4px;
  max-height: 200px;
  overflow-y: auto;
  white-space: pre-wrap;
  line-height: 1.6;
}

.input-area {
  padding: 14px 28px;
  background: var(--el-bg-color);
  border-top: 1px solid var(--el-border-color-light);
  display: flex;
  gap: 12px;
  align-items: flex-end;
  flex-shrink: 0;
}

.input-area :deep(.el-textarea__inner) {
  flex: 1;
}
</style>
