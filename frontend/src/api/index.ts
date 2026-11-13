import axios from "axios";
import { API_BASE, createHttp } from "./http";
import { useAuthStore } from "../stores/auth";

/// refresh_token 本地键（auth store 同款）
export const REFRESH_KEY = "rustplm_refresh";

/// 共享 axios 单例（token 取自 localStorage；401 由 http.ts refresh 单飞自愈）
export const http = createHttp(() => localStorage.getItem("rustplm_token"), {
  // 裸 axios 调 refresh 端点（不经本实例——避免拦截器循环）
  onRefresh: async () => {
    const rt = localStorage.getItem(REFRESH_KEY);
    if (!rt) return null;
    try {
      const res = await axios.post(`${API_BASE}/api/v1/auth/refresh`, { refresh_token: rt }, { timeout: 15000 });
      const pair = res.data as { access_token: string; refresh_token: string };
      localStorage.setItem("rustplm_token", pair.access_token);
      localStorage.setItem(REFRESH_KEY, pair.refresh_token);
      // 同步 store（终审 M1：避免 store.token 与 localStorage 脱钩）
      useAuthStore().setToken(pair.access_token);
      return pair.access_token;
    } catch {
      return null;
    }
  },
});
