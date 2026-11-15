import { createRouter, createWebHistory } from "vue-router";

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: "/login", component: () => import("../pages/Login.vue"), meta: { public: true } },
    // 主布局壳（增量20 终审前清偿：MainLayout 原为死组件——顶栏全局搜索/侧边菜单接线生效；
    // 子路径与既有 URL 完全一致，无路由破坏）
    {
      path: "/",
      component: () => import("../layouts/MainLayout.vue"),
      children: [
        { path: "", component: () => import("../pages/Workbench.vue") },
        { path: "docs", component: () => import("../pages/DocsList.vue") },
        { path: "docs/:objNo", component: () => import("../pages/DocDetail.vue") },
        // 待办中心（增量19 WF-03：待办/已办/我发起 + 批量审批）
        { path: "todos", component: () => import("../pages/Todo.vue") },
        // 全局搜索（增量20 RPT-01：一处搜一切、按类型聚类）
        { path: "search", component: () => import("../pages/Search.vue") },
        // 物料主数据（增量17）
        { path: "parts", component: () => import("../pages/Parts.vue") },
        // 用户管理（增量11；后端 sec:manage 门控——无权限者 API 403 提示）
        { path: "admin/users", component: () => import("../pages/UserAdmin.vue") },
        // 审计日志（增量18 SYS-01）
        { path: "admin/audit", component: () => import("../pages/Audit.vue") },
      ],
    },
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
