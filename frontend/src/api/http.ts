import axios, { AxiosError, type AxiosInstance, type AxiosRequestConfig, type InternalAxiosRequestConfig } from "axios";

export const API_BASE = import.meta.env.VITE_API_BASE ?? "http://localhost:8080";

export interface ApiErrorBody {
  code: string;
  message: string;
  trace_id: string;
}

let onUnauthorized: (() => void) | null = null;

/// 由 router/store 注册（401 无法自愈 → 清 token 跳登录，计划 Review Focus #1）
export function setUnauthorizedHandler(fn: () => void) {
  onUnauthorized = fn;
}

export interface HttpOptions {
  /// 401 自愈：用 refresh_token 换新 access（增量12 单飞接线）；null=放弃（跳登录）
  onRefresh?: () => Promise<string | null>;
  /// 测试注入（axios adapter）
  adapter?: AxiosRequestConfig["adapter"];
}

export function createHttp(
  tokenGetter: () => string | null,
  opts: HttpOptions = {},
): AxiosInstance {
  const http = axios.create({ baseURL: API_BASE, timeout: 30000 });
  if (opts.adapter) (http.defaults as unknown as { adapter: AxiosRequestConfig["adapter"] }).adapter = opts.adapter;

  http.interceptors.request.use((cfg) => {
    const t = tokenGetter();
    if (t) cfg.headers.set("authorization", `Bearer ${t}`);
    return cfg;
  });

  // refresh 单飞（增量12 M9）：并发 401 共享一次换发；换发成功重放原请求，
  // 失败/无 refresh/auth 路由自身 401 → onUnauthorized 跳登录
  let refreshing: Promise<string | null> | null = null;

  http.interceptors.response.use(
    (res) => res,
    async (err: AxiosError<ApiErrorBody>) => {
      const cfg = err.config as (InternalAxiosRequestConfig & { _retry?: boolean }) | undefined;
      const isAuthRoute = String(cfg?.url ?? "").includes("/auth/");
      if (err.response?.status === 401 && cfg && !cfg._retry && !isAuthRoute && opts.onRefresh) {
        if (!refreshing) {
          refreshing = opts
            .onRefresh()
            .finally(() => {
              refreshing = null;
            });
        }
        const newToken = await refreshing;
        if (newToken) {
          cfg._retry = true;
          cfg.headers.set("authorization", `Bearer ${newToken}`);
          return http.request(cfg);
        }
      }
      if (err.response?.status === 401) {
        onUnauthorized?.();
      }
      return Promise.reject(err);
    },
  );
  return http;
}

/// 统一错误体提取（ADR-0015）；非 API 错误降级原消息
export function extractApiError(err: unknown): string {
  const e = err as AxiosError<ApiErrorBody>;
  return e.response?.data?.message ?? (e as Error).message ?? "未知错误";
}

export function extractApiCode(err: unknown): string | undefined {
  const e = err as AxiosError<ApiErrorBody>;
  return e.response?.data?.code;
}
