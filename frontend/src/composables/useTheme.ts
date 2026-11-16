// 主题机制（增量29）：light/dark 切换 + localStorage 持久化 + html.dark 驱动
// （Element dark css-vars 与自研 tokens 双层跟随）。演示确定性优先：无 auto 档（台账记）。

export type Theme = "light" | "dark";

const STORAGE_KEY = "rustplm-theme";

/// 从存储读主题（纯函数可单测；非法值回退 light）
export function themeFromStorage(raw: string | null): Theme {
  return raw === "dark" ? "dark" : "light";
}

/// 应用主题到 DOM（html.dark class——Element dark css-vars 与 tokens 双层跟随）
export function applyTheme(theme: Theme): void {
  const el = document.documentElement;
  if (theme === "dark") {
    el.classList.add("dark");
  } else {
    el.classList.remove("dark");
  }
}

/// 初始化：读存储并应用（App 启动调用一次）
export function initTheme(): Theme {
  const theme = themeFromStorage(localStorage.getItem(STORAGE_KEY));
  applyTheme(theme);
  return theme;
}

/// 切换并持久化（返回新主题）
export function toggleTheme(): Theme {
  const current = themeFromStorage(localStorage.getItem(STORAGE_KEY));
  const next: Theme = current === "dark" ? "light" : "dark";
  localStorage.setItem(STORAGE_KEY, next);
  applyTheme(next);
  return next;
}
