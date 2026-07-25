// 知识库 Pinia Store

import { defineStore } from "pinia";
import { ref } from "vue";
import { api, type KnowledgeBase, type CreateKnowledgeBaseRequest } from "@/api/invoke";
import { ElMessage, ElMessageBox } from "element-plus";

export const useKbStore = defineStore("kb", () => {
  const knowledgeBases = ref<KnowledgeBase[]>([]);
  const loading = ref(false);
  const currentKb = ref<KnowledgeBase | null>(null);

  /** 加载知识库列表 */
  async function loadKnowledgeBases() {
    loading.value = true;
    try {
      knowledgeBases.value = await api.getKnowledgeBases();
    } catch (e) {
      ElMessage.error(`加载知识库失败: ${e}`);
    } finally {
      loading.value = false;
    }
  }

  /** 创建知识库 */
  async function createKnowledgeBase(req: CreateKnowledgeBaseRequest) {
    try {
      const kb = await api.createKnowledgeBase(req);
      knowledgeBases.value.unshift(kb);
      ElMessage.success(`知识库「${kb.name}」创建成功`);
      return kb;
    } catch (e) {
      ElMessage.error(`创建失败: ${e}`);
      throw e;
    }
  }

  /** 删除知识库 */
  async function deleteKnowledgeBase(kb: KnowledgeBase) {
    try {
      await ElMessageBox.confirm(
        `确定删除知识库「${kb.name}」？该操作将删除所有文档和向量索引，不可恢复。`,
        "删除确认",
        { type: "warning", confirmButtonText: "删除", cancelButtonText: "取消" }
      );
      await api.deleteKnowledgeBase(kb.id);
      knowledgeBases.value = knowledgeBases.value.filter((k) => k.id !== kb.id);
      if (currentKb.value?.id === kb.id) currentKb.value = null;
      ElMessage.success("知识库已删除");
    } catch (e) {
      if (e !== "cancel") ElMessage.error(`删除失败: ${e}`);
    }
  }

  /** 重命名知识库 */
  async function renameKnowledgeBase(kb: KnowledgeBase) {
    try {
      const { value } = await ElMessageBox.prompt("请输入新的知识库名称", "重命名", {
        confirmButtonText: "确定",
        cancelButtonText: "取消",
        inputValue: kb.name,
        inputValidator: (v: string) => v.trim().length > 0 || "名称不能为空",
      });
      const newName = value.trim();
      if (newName === kb.name) return;
      await api.renameKnowledgeBase(kb.id, newName);
      // 更新本地列表
      const target = knowledgeBases.value.find((k) => k.id === kb.id);
      if (target) target.name = newName;
      if (currentKb.value?.id === kb.id) currentKb.value.name = newName;
      ElMessage.success("知识库已重命名");
    } catch (e) {
      if (e !== "cancel") ElMessage.error(`重命名失败: ${e}`);
    }
  }

  /** 设置当前知识库 */
  function setCurrentKb(kb: KnowledgeBase | null) {
    currentKb.value = kb;
  }

  return {
    knowledgeBases,
    loading,
    currentKb,
    loadKnowledgeBases,
    createKnowledgeBase,
    deleteKnowledgeBase,
    renameKnowledgeBase,
    setCurrentKb,
  };
});
