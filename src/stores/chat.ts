// 对话 Pinia Store

import { defineStore } from "pinia";
import { ref, computed } from "vue";
import {
  api,
  events,
  type Conversation,
  type Message,
  type RetrievedChunk,
  type QueryOptions,
} from "@/api/invoke";
import { ElMessage } from "element-plus";
import { v4 as uuidv4 } from "uuid";
import type { UnlistenFn } from "@tauri-apps/api/event";

export interface ChatMessage {
  id: string;
  role: "user" | "assistant" | "system";
  content: string;
  references?: RetrievedChunk[];
  pending?: boolean;
  createdAt: number;
}

export const useChatStore = defineStore("chat", () => {
  const conversations = ref<Conversation[]>([]);
  const currentConversation = ref<Conversation | null>(null);
  const messages = ref<ChatMessage[]>([]);
  const loading = ref(false);
  const querying = ref(false);

  /** 当前活跃查询的标识:切换 KB/对话时置空,使旧查询的流式事件失效(防串扰) */
  const activeQueryId = ref<string | null>(null);

  // 查询参数
  const queryOptions = ref<QueryOptions>({
    top_k: 4,
    temperature: 0.7,
    max_tokens: 1024,
    use_history: true,
  });

  const hasMessages = computed(() => messages.value.length > 0);

  /** 加载某知识库下的对话列表 */
  async function loadConversations(kbId: string) {
    loading.value = true;
    // 切换知识库时清空当前对话和消息,避免显示上一个 KB 的内容
    currentConversation.value = null;
    messages.value = [];
    // 放弃上一个 KB 可能仍在进行的查询:其流式事件将被忽略
    activeQueryId.value = null;
    querying.value = false;
    try {
      conversations.value = await api.getConversations(kbId);
    } catch (e) {
      ElMessage.error(`加载对话列表失败: ${e}`);
    } finally {
      loading.value = false;
    }
  }

  /** 新建对话(本地占位,保存时落库) */
  function newConversation(kbId: string): Conversation {
    const now = new Date().toISOString();
    const conv: Conversation = {
      id: uuidv4(),
      knowledge_base_id: kbId,
      title: "新对话",
      created_at: now,
      updated_at: now,
    };
    currentConversation.value = conv;
    messages.value = [];
    // 新建对话同样放弃进行中的查询(ask 内部调用本函数时尚未发起新查询,复位安全)
    activeQueryId.value = null;
    querying.value = false;
    conversations.value.unshift(conv);
    return conv;
  }

  /** 切换到指定对话 */
  async function selectConversation(conv: Conversation) {
    currentConversation.value = conv;
    messages.value = [];
    // 切换对话时放弃进行中的查询(其 done 事件的标题/保存逻辑不应作用于新对话)
    activeQueryId.value = null;
    querying.value = false;
    try {
      const msgs = await api.getMessages(conv.id);
      messages.value = msgs.map((m) => ({
        id: m.id,
        role: m.role as ChatMessage["role"],
        content: m.content,
        references: safeParseReferences(m.references),
        createdAt: new Date(m.created_at).getTime(),
      }));
    } catch (e) {
      ElMessage.error(`加载消息失败: ${e}`);
    }
  }

  /** 发起问答(流式) */
  async function ask(kbId: string, question: string) {
    if (!question.trim() || querying.value) return;
    if (!currentConversation.value) {
      newConversation(kbId);
    }

    querying.value = true;
    const queryId = uuidv4();
    activeQueryId.value = queryId;

    // 追加用户消息
    const userMsg: ChatMessage = {
      id: uuidv4(),
      role: "user",
      content: question,
      createdAt: Date.now(),
    };
    messages.value.push(userMsg);

    // 追加占位助手消息
    const assistantMsg: ChatMessage = {
      id: uuidv4(),
      role: "assistant",
      content: "",
      pending: true,
      createdAt: Date.now(),
    };
    messages.value.push(assistantMsg);

    // 获取 Vue 响应式代理引用(直接修改 assistantMsg 不会触发 UI 更新)
    const msgIndex = messages.value.length - 1;
    const reactiveMsg = messages.value[msgIndex];

    // 注册流式事件监听
    // 双重校验:payload 须属于本次查询,且本次查询仍是活跃查询(未被切换 KB/对话放弃)
    const isActive = (payloadQueryId?: string | null) =>
      payloadQueryId === queryId && activeQueryId.value === queryId;

    let unlistenToken: UnlistenFn | null = null;
    let unlistenDone: UnlistenFn | null = null;

    try {
      unlistenToken = await events.onChatToken((payload) => {
        if (!isActive(payload.query_id)) return;
        // 逐 token 追加到助手消息(通过响应式引用)
        reactiveMsg.content += payload.token;
      });

      unlistenDone = await events.onChatDone((payload) => {
        if (!isActive(payload.query_id)) return;
        activeQueryId.value = null;
        reactiveMsg.pending = false;
        if (payload.error) {
          reactiveMsg.content = `查询失败: ${payload.error}`;
          ElMessage.error(`查询失败: ${payload.error}`);
        } else {
          // 仅当后端返回完整答案且非空时才覆盖(防止 token 拼接遗漏)
          if (payload.answer) {
            reactiveMsg.content = payload.answer;
          }
          reactiveMsg.references = payload.references;

          // 若是新对话,用问题作标题
          if (
            currentConversation.value &&
            currentConversation.value.title === "新对话"
          ) {
            currentConversation.value.title =
              question.slice(0, 30) + (question.length > 30 ? "..." : "");
          }
          // 自动保存
          saveCurrentConversation();
        }
        querying.value = false;
      });

      // 构造历史(不含刚追加的用户消息和占位助手消息)
      const history = messages.value
        .slice(0, -2)
        .filter((m) => !m.pending && m.content)
        .map((m) => ({ role: m.role, content: m.content }));

      await api.queryKnowledgeBaseStream({
        kb_id: kbId,
        question,
        query_id: queryId,
        history,
        options: queryOptions.value,
      });
    } catch (e) {
      activeQueryId.value = null;
      reactiveMsg.content = `查询失败: ${e}`;
      reactiveMsg.pending = false;
      ElMessage.error(`查询失败: ${e}`);
      querying.value = false;
    } finally {
      // 清理监听器
      unlistenToken?.();
      unlistenDone?.();
    }
  }

  /** 保存当前对话 */
  async function saveCurrentConversation() {
    if (!currentConversation.value) return;
    const conv = currentConversation.value;
    const now = new Date().toISOString();
    conv.updated_at = now;

    const msgs: Message[] = messages.value
      .filter((m) => !m.pending && m.content)
      .map((m) => ({
        id: m.id,
        conversation_id: conv.id,
        role: m.role,
        content: m.content,
        references: JSON.stringify(m.references || []),
        created_at: new Date(m.createdAt).toISOString(),
      }));

    try {
      await api.saveConversation({ conversation: conv, messages: msgs });
    } catch (e) {
      console.error("保存对话失败", e);
    }
  }

  /** 删除对话 */
  async function deleteConversation(conv: Conversation) {
    try {
      await api.deleteConversation(conv.id);
      conversations.value = conversations.value.filter((c) => c.id !== conv.id);
      if (currentConversation.value?.id === conv.id) {
        currentConversation.value = null;
        messages.value = [];
      }
      ElMessage.success("对话已删除");
    } catch (e) {
      ElMessage.error(`删除失败: ${e}`);
    }
  }

  /** 清空消息 */
  function clearMessages() {
    messages.value = [];
  }

  function safeParseReferences(s: string): RetrievedChunk[] {
    try {
      const v = JSON.parse(s);
      return Array.isArray(v) ? v : [];
    } catch {
      return [];
    }
  }

  return {
    conversations,
    currentConversation,
    messages,
    loading,
    querying,
    queryOptions,
    hasMessages,
    loadConversations,
    newConversation,
    selectConversation,
    ask,
    saveCurrentConversation,
    deleteConversation,
    clearMessages,
  };
});
