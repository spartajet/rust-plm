import axios, { AxiosError, type AxiosInstance } from "axios";

export const API_BASE = import.meta.env.VITE_API_BASE ?? "http://localhost:8080";

export interface ApiErrorBody {
  code: string;
  message: string;
  trace_id: string;
}

let onUnauthorized: (() => void) | null = null;

/// 由 router/store 注册（401 → 清 token 跳登录，计划 Review Focus #1）
export function setUnauthorizedHandler(fn: () => void) {
  onUnauthorized = fn;
}

export function createHttp(tokenGetter: () => string | null): AxiosInstance {
  const http = axios.create({ baseURL: API_BASE, timeout: 30000 });

  http.interceptors.request.use((cfg) => {
    const t = tokenGetter();
    if (t) cfg.headers.set("authorization", `Bearer ${t}`);
    return cfg;
  });

  http.interceptors.response.use(
    (res) => res,
    (err: AxiosError<ApiErrorBody>) => {
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
