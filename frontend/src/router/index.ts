import { createRouter, createWebHistory } from "vue-router";

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: "/login", component: () => import("../pages/Login.vue"), meta: { public: true } },
    { path: "/", component: () => import("../pages/Workbench.vue") },
    { path: "/docs", component: () => import("../pages/DocsList.vue") },
    { path: "/docs/:objNo", component: () => import("../pages/DocDetail.vue") },
    // 用户管理（增量11；后端 sec:manage 门控——无权限者 API 403 提示）
    { path: "/admin/users", component: () => import("../pages/UserAdmin.vue") },
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
