import { defineStore } from "pinia";

const TOKEN_KEY = "rustplm_token";
const USER_KEY = "rustplm_user";
export const REFRESH_KEY = "rustplm_refresh";
export const ROLES_KEY = "rustplm_roles";

/// 角色→能力映射（与后端 RBAC 种子 0005 一致；自定义角色后置记账——增量12）
const ROLE_CAPS: Record<string, string[]> = {
  admin: ["docs:read", "docs:write", "docs:approve", "sec:manage"],
  editor: ["docs:read", "docs:write"],
  viewer: ["docs:read"],
};

export const useAuthStore = defineStore("auth", {
  state: () => ({
    token: localStorage.getItem(TOKEN_KEY) ?? "",
    username: localStorage.getItem(USER_KEY) ?? "",
    roles: (() => {
      // 存储损坏兜底（终审 M3：JSON.parse 抛错=白屏）
      try {
        return JSON.parse(localStorage.getItem(ROLES_KEY) ?? "[]") as string[];
      } catch {
        return [];
      }
    })(),
  }),
  getters: {
    isAuthenticated: (s) => s.token.length > 0,
    /// 能力集合（M8 显隐：由 roles 推导；后端 casbin 仍是权威门）
    capabilities(): Set<string> {
      const caps = new Set<string>();
      for (const r of this.roles) for (const c of ROLE_CAPS[r] ?? []) caps.add(c);
      return caps;
    },
    canWrite(): boolean {
      return this.capabilities.has("docs:write");
    },
    canApprove(): boolean {
      return this.capabilities.has("docs:approve");
    },
    canManageSec(): boolean {
      return this.capabilities.has("sec:manage");
    },
  },
  actions: {
    setSession(token: string, username: string, refreshToken?: string, roles?: string[]) {
      this.token = token;
      this.username = username;
      this.roles = roles ?? [];
      localStorage.setItem(TOKEN_KEY, token);
      localStorage.setItem(USER_KEY, username);
      if (refreshToken !== undefined) localStorage.setItem(REFRESH_KEY, refreshToken);
      if (roles !== undefined) localStorage.setItem(ROLES_KEY, JSON.stringify(roles));
    },
    /** 仅更新 access（refresh 单飞换发后由拦截器调用——不经 store 避免循环） */
    setToken(token: string) {
      this.token = token;
      localStorage.setItem(TOKEN_KEY, token);
    },
    clear() {
      this.token = "";
      this.username = "";
      this.roles = [];
      localStorage.removeItem(TOKEN_KEY);
      localStorage.removeItem(USER_KEY);
      localStorage.removeItem(REFRESH_KEY);
      localStorage.removeItem(ROLES_KEY);
    },
  },
});
