import { createRouter, createWebHistory } from "vue-router";

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: "/login", component: () => import("../pages/Login.vue"), meta: { public: true } },
    { path: "/", component: () => import("../pages/Workbench.vue") },
    { path: "/docs", component: () => import("../pages/DocsList.vue") },
    { path: "/docs/:objNo", component: () => import("../pages/DocDetail.vue") },
    // 待办中心（增量19 WF-03：待办/已办/我发起 + 批量审批）
    { path: "/todos", component: () => import("../pages/Todo.vue") },
    // 物料主数据（增量17）
    { path: "/parts", component: () => import("../pages/Parts.vue") },
    // 用户管理（增量11；后端 sec:manage 门控——无权限者 API 403 提示）
    { path: "/admin/users", component: () => import("../pages/UserAdmin.vue") },
    // 审计日志（增量18 SYS-01）
    { path: "/admin/audit", component: () => import("../pages/Audit.vue") },
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
