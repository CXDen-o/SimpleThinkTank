import { createRouter, createWebHistory } from "vue-router";

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: "/",
      redirect: "/knowledge-bases",
    },
    {
      path: "/knowledge-bases",
      name: "knowledge-bases",
      component: () => import("@/views/KnowledgeBases.vue"),
    },
    {
      path: "/kb/:id/documents",
      name: "documents",
      component: () => import("@/views/Documents.vue"),
      props: true,
    },
    {
      path: "/kb/:id/chat",
      name: "chat",
      component: () => import("@/views/Chat.vue"),
      props: true,
    },
    {
      path: "/settings",
      name: "settings",
      component: () => import("@/views/Settings.vue"),
    },
  ],
});

export default router;
