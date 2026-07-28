// Tauri Command 调用封装层
// 类型安全的前后端通信

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ===== 类型定义 =====

export interface KnowledgeBase {
  id: string;
  name: string;
  description: string | null;
  storage_path: string;
  split_strategy: string;
  split_config: string;
  created_at: string;
  updated_at: string;
}

export interface CreateKnowledgeBaseRequest {
  name: string;
  description?: string;
  storage_path?: string;
  split_strategy?: string;
  split_config?: string;
}

export interface Document {
  id: string;
  knowledge_base_id: string;
  file_name: string;
  file_path: string;
  file_size: number;
  file_type: string;
  content_hash: string | null;
  status: string;
  chunk_count: number;
  error_message: string | null;
  created_at: string;
  updated_at: string;
}

export interface ImportDocumentsRequest {
  knowledge_base_id: string;
  file_paths: string[];
}

export interface ImportProgress {
  task_id: string;
  total: number;
  completed: number;
  failed: number;
  current_file: string | null;
  status: string;
}

export interface SystemInfo {
  ollama_installed: boolean;
  ollama_running: boolean;
  default_models_available: boolean;
}

export interface StrategyParamSchema {
  key: string;
  label: string;
  type: string; // "number" | "string" | "boolean"
  default: any;
  min: number | null;
  max: number | null;
}

export interface StrategyInfo {
  id: string;
  name: string;
  description: string;
  config_schema: StrategyParamSchema[];
}

export interface ChunkResult {
  text: string;
  start: number;
  end: number;
  index: number;
  metadata: any;
}

export interface RetrievedChunk {
  chunk_id: string;
  document_id: string;
  content: string;
  score: number;
  metadata: any;
}

export interface QueryOptions {
  top_k?: number;
  temperature?: number;
  max_tokens?: number;
  use_history?: boolean;
}

export interface QueryRequest {
  kb_id: string;
  question: string;
  /** 流式查询标识:前端生成,随事件透传,用于区分新旧查询 */
  query_id?: string;
  history?: HistoryMessage[];
  options?: QueryOptions;
}

export interface HistoryMessage {
  role: string;
  content: string;
}

export interface QueryResponse {
  answer: string;
  references: RetrievedChunk[];
}

export interface Conversation {
  id: string;
  knowledge_base_id: string;
  title: string;
  created_at: string;
  updated_at: string;
}

export interface Message {
  id: string;
  conversation_id: string;
  role: string;
  content: string;
  references: string;
  created_at: string;
}

export interface SaveConversationRequest {
  conversation: Conversation;
  messages: Message[];
}

export interface ModelDownloadProgress {
  model: string;
  status: string; // pulling / success / error / cancelled / retrying
  completed: number | null;
  total: number | null;
}

/** 增强版下载进度 payload */
export interface DownloadProgressPayload {
  model: string;
  status: string; // pulling / success / error / cancelled / retrying
  completed: number | null;
  total: number | null;
  percent: number | null; // 0.0 ~ 1.0
  rate_bps: number | null; // 字节/秒
  eta_secs: number | null; // 秒
  attempt: number;
  max_attempts: number;
  error: string | null;
}

/** 应用设置(对应后端 AppSettings) */
export interface AppSettings {
  ollama_base_url: string;
  ollama_registry: string;
  https_proxy: string;
  use_custom_models_dir: boolean;
  download_max_retries: number;
  download_connect_timeout_secs: number;
  download_request_timeout_secs: number;
  query_options: string;
  /** 对话模型名,空串表示默认 qwen3:1.7b(嵌入模型全局锁定) */
  chat_model: string;
}

/** 测试下载源请求 */
export interface TestSourceRequest {
  registry?: string | null;
  proxy?: string | null;
}

/** 测试下载源结果 */
export interface TestSourceResult {
  ok: boolean;
  latency_ms: number;
  message: string;
}

/** 文件系统层模型存在性检查结果 */
export interface ModelsOnDisk {
  /** 当前生效的对话模型名 */
  chat_model: string;
  /** 生效对话模型是否已在磁盘 */
  chat_model_installed: boolean;
  /** 嵌入模型(锁定)是否已在磁盘 */
  embedding_model_installed: boolean;
  all_installed: boolean;
  /** 文件系统层发现的全部对话模型(已过滤嵌入模型) */
  local_chat_models: string[];
  scanned_dirs: string[];
}

/** 推荐对话模型条目(对应后端 RecommendedChatModel) */
export interface RecommendedChatModel {
  name: string;
  tier_label: string;
  size_hint: string;
}

/** 单个知识库的存储统计 */
export interface KbStorageStat {
  id: string;
  name: string;
  document_count: number;
  chunk_count: number;
  vector_count: number;
  /** 原文件大小合计(documents.file_size) */
  documents_bytes: number;
  /** KB 目录实际磁盘占用 */
  dir_bytes: number;
}

/** 单个 Ollama 模型目录占用 */
export interface ModelDirStat {
  path: string;
  exists: boolean;
  bytes: number;
}

/** 存储统计总览 */
export interface StorageStats {
  /** simplethinktank.db(含 -wal/-shm) */
  database_bytes: number;
  /** knowledge_bases/ 目录总占用 */
  knowledge_bases_bytes: number;
  /** logs/ 目录占用 */
  logs_bytes: number;
  /** 应用自身占用合计(不含模型) */
  total_bytes: number;
  model_dirs: ModelDirStat[];
  knowledge_bases: KbStorageStat[];
}

// ===== API 封装 =====

export const api = {
  // 知识库
  getKnowledgeBases: () => invoke<KnowledgeBase[]>("get_knowledge_bases"),

  createKnowledgeBase: (req: CreateKnowledgeBaseRequest) =>
    invoke<KnowledgeBase>("create_knowledge_base", { req }),

  deleteKnowledgeBase: (kbId: string) =>
    invoke<void>("delete_knowledge_base", { kbId }),

  renameKnowledgeBase: (kbId: string, newName: string) =>
    invoke<void>("rename_knowledge_base", { kbId, newName }),

  // 文档
  importDocuments: (req: ImportDocumentsRequest) =>
    invoke<string>("import_documents", { req }),

  getDocuments: (kbId: string) => invoke<Document[]>("get_documents", { kbId }),

  getImportTaskProgress: (taskId: string) =>
    invoke<ImportProgress | null>("get_import_task_progress", { taskId }),

  /** 取消导入任务(当前文件处理完后停止) */
  cancelImport: (taskId: string) => invoke<boolean>("cancel_import", { taskId }),

  /** 删除文档(连带清理 chunks 和向量) */
  deleteDocument: (docId: string) => invoke<void>("delete_document", { docId }),

  // 切分策略
  getSplitStrategies: () => invoke<StrategyInfo[]>("get_split_strategies"),

  previewSplit: (text: string, strategyId: string, params: any) =>
    invoke<ChunkResult[]>("preview_split", { text, strategyId, params }),

  /** 列出本机已安装的 Ollama 模型(供切分模型下拉选择) */
  listLocalModels: () => invoke<string[]>("list_local_models"),

  // RAG 对话
  queryKnowledgeBase: (req: QueryRequest) =>
    invoke<QueryResponse>("query_knowledge_base", { req }),

  queryKnowledgeBaseStream: (req: QueryRequest) =>
    invoke<void>("query_knowledge_base_stream", { req }),

  getConversations: (kbId: string) =>
    invoke<Conversation[]>("get_conversations", { kbId }),

  getMessages: (conversationId: string) =>
    invoke<Message[]>("get_messages", { conversationId }),

  saveConversation: (req: SaveConversationRequest) =>
    invoke<void>("save_conversation", { req }),

  deleteConversation: (conversationId: string) =>
    invoke<void>("delete_conversation", { conversationId }),

  // 系统 / Ollama
  getSystemInfo: () => invoke<SystemInfo>("get_system_info"),

  installOllama: () => invoke<string>("install_ollama"),

  startOllama: () => invoke<void>("start_ollama"),

  downloadDefaultModels: () => invoke<void>("download_default_models"),

  cancelModelDownload: (model: string) =>
    invoke<boolean>("cancel_model_download", { model }),

  getAppSettings: () => invoke<AppSettings>("get_app_settings"),

  updateAppSettings: (req: AppSettings) =>
    invoke<void>("update_app_settings", { req }),

  testDownloadSource: (req: TestSourceRequest) =>
    invoke<TestSourceResult>("test_download_source", { req }),

  checkModelsOnDisk: () => invoke<ModelsOnDisk>("check_models_on_disk"),

  /** 获取推荐对话模型候选表(静态配置) */
  getRecommendedChatModels: () =>
    invoke<RecommendedChatModel[]>("get_recommended_chat_models"),

  getStorageStats: () => invoke<StorageStats>("get_storage_stats"),

  clearLogs: () => invoke<number>("clear_logs"),

  shutdownCleanup: (forceOllamaStop: boolean) =>
    invoke<string[]>("shutdown_cleanup", { forceOllamaStop }),
};

// ===== 事件监听 =====

export const events = {
  /** 监听文档导入进度 */
  onImportProgress: (
    handler: (progress: ImportProgress) => void
  ): Promise<UnlistenFn> => {
    return listen<ImportProgress>("import-progress", (event) => {
      handler(event.payload);
    });
  },

  /** 监听模型下载进度(增强版,含速率/ETA/重试信息) */
  onModelDownloadProgress: (
    handler: (progress: DownloadProgressPayload) => void
  ): Promise<UnlistenFn> => {
    return listen<DownloadProgressPayload>("model-download-progress", (event) => {
      handler(event.payload);
    });
  },

  /** 监听问答参数变更(来自设置窗口) */
  onQueryOptionsChanged: (
    handler: (options: QueryOptions) => void
  ): Promise<UnlistenFn> => {
    return listen<QueryOptions>("query-options-changed", (event) => {
      handler(event.payload);
    });
  },

  /** 流式问答 token 事件 */
  onChatToken: (handler: (payload: ChatTokenPayload) => void): Promise<UnlistenFn> => {
    return listen<ChatTokenPayload>("chat-token", (event) => {
      handler(event.payload);
    });
  },

  /** 流式问答完成事件 */
  onChatDone: (handler: (payload: ChatDonePayload) => void): Promise<UnlistenFn> => {
    return listen<ChatDonePayload>("chat-done", (event) => {
      handler(event.payload);
    });
  },
};

/** 流式问答 token 事件 payload */
export interface ChatTokenPayload {
  token: string;
  /** 关联的查询标识 */
  query_id?: string | null;
}

/** 流式问答完成事件 payload */
export interface ChatDonePayload {
  answer: string;
  references: RetrievedChunk[];
  error: string | null;
  /** 关联的查询标识 */
  query_id?: string | null;
}
