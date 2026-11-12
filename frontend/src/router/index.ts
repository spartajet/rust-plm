import { createRouter, createWebHistory } from "vue-router";

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: "/login", component: () => import("../pages/Login.vue"), meta: { public: true } },
    { path: "/", component: () => import("../pages/Workbench.vue") },
    { path: "/docs", component: () => import("../pages/DocsList.vue") },
    { path: "/docs/:objNo", component: () => import("../pages/DocDetail.vue") },
  ],
});

/// 路由守卫纯逻辑（可单测）：未认证访问非公开页 → 跳登录
export function guardTarget(
  isAuthenticated: boolean,
  toMetaPublic: boolean | undefined,
): string | null {
  if (!isAuthenticated && !toMetaPublic) return "/login";
  return null;
}
