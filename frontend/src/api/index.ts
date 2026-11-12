import { createHttp } from "./http";
import { useAuthStore } from "../stores/auth";

/// 共享 axios 单例（token 取自 auth store；401 由 http.ts 统一处理）
export const http = createHttp(() => localStorage.getItem("rustplm_token"));

// 防循环依赖：store 在 http 之后创建（main.ts 顺序保证）
export function authStore() {
  return useAuthStore();
}
