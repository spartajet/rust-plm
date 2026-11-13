import { describe, expect, it } from "vitest";
import { flowUi, stateLabel } from "../src/utils/flowUi";
import { guardTarget } from "../src/router/index";
import { buildListQuery, parseNdjson } from "../src/api/docs";

// Review Focus #2/#1：flowUi 全状态穷举（增量8 +revise 维度；增量11 +checked 态——strict 二级）
describe("flowUi", () => {
  it("working: submit+obsolete only", () => {
    expect(flowUi("working")).toEqual({ submit: true, approve: false, reject: false, obsolete: true, revise: false });
  });
  it("review: approve+reject only", () => {
    expect(flowUi("review")).toEqual({ submit: false, approve: true, reject: true, obsolete: false, revise: false });
  });
  it("checked: approve+reject only (strict 二级，M2 清偿)", () => {
    expect(flowUi("checked")).toEqual({ submit: false, approve: true, reject: true, obsolete: false, revise: false });
  });
  it("released: obsolete+revise only", () => {
    expect(flowUi("released")).toEqual({ submit: false, approve: false, reject: false, obsolete: true, revise: true });
  });
  it("obsolete: all off", () => {
    expect(flowUi("obsolete")).toEqual({ submit: false, approve: false, reject: false, obsolete: false, revise: false });
  });
  it("unknown state: all off", () => {
    expect(flowUi("nonsense")).toEqual({ submit: false, approve: false, reject: false, obsolete: false, revise: false });
  });
  it("labels", () => {
    expect(stateLabel("working")).toBe("工作中");
    expect(stateLabel("released")).toBe("已发布");
    expect(stateLabel("checked")).toBe("待复核");
    expect(stateLabel("x")).toBe("x");
  });
});

// Review Focus #3：列表查询拼装与后端契约一致
describe("buildListQuery", () => {
  it("material filter + paging", () => {
    const q = decodeURIComponent(buildListQuery({ material: "Q235", page: 2, size: 15 }));
    expect(q).toContain("filter[material]=Q235");
    expect(q).toContain("page=2");
    expect(q).toContain("size=15");
  });
  it("state filter + sort", () => {
    const q = decodeURIComponent(buildListQuery({ state: "released", sort: "-updated_at" }));
    expect(q).toContain("filter[state]=released");
    expect(q).toContain("sort=-updated_at");
  });
  it("defaults page/size", () => {
    const q = buildListQuery({});
    expect(q).toContain("page=1");
    expect(q).toContain("size=20");
  });
});

// 路由守卫纯逻辑
describe("guardTarget", () => {
  it("unauthenticated → /login", () => {
    expect(guardTarget(false, false)).toBe("/login");
  });
  it("public page always allowed", () => {
    expect(guardTarget(false, true)).toBeNull();
  });
  it("authenticated passes", () => {
    expect(guardTarget(true, false)).toBeNull();
  });
});

// NDJSON 解析（BOM 展开行）
describe("parseNdjson", () => {
  it("parses lines and skips blanks", () => {
    const rows = parseNdjson('{"child":"a","qty_total":6}\n\n{"child":"b","qty_total":1}');
    expect(rows).toHaveLength(2);
    expect(rows[0]["child"]).toBe("a");
  });
  it("empty text → empty", () => {
    expect(parseNdjson("")).toEqual([]);
  });
});

// ==================== 增量12：上传预校验 + stateTagType ====================
import { validateUpload } from "../src/utils/upload";
import { stateTagType } from "../src/utils/flowUi";

describe("validateUpload（增量7 M2 清偿：客户端预校验）", () => {
  const f = (name: string, size: number) => ({ name, size }) as File;
  it("空文件拒绝", () => {
    const r = validateUpload(f("a.dwg", 0));
    expect(r.ok).toBe(false);
  });
  it("超大拒绝（>200MB）", () => {
    const r = validateUpload(f("big.step", 201 * 1024 * 1024));
    expect(r.ok).toBe(false);
  });
  it("常规图纸通过", () => {
    expect(validateUpload(f("支架.dwg", 1024)).ok).toBe(true);
    expect(validateUpload(f("part.step", 1024)).ok).toBe(true);
    expect(validateUpload({ name: "无扩展名", size: 1 } as unknown as File).ok).toBe(true); // 白名单外仅告警不阻断
  });
  it("白名单外仅 WARN（不阻断——图纸长尾格式开放裁定）", () => {
    const r = validateUpload(f("data.xyz", 1024));
    expect(r.ok).toBe(true);
    expect(r.warn).toBe(true);
  });
});

describe("stateTagType（终审 R1 回归钉）", () => {
  it("checked 为警示色（非红）", () => {
    expect(stateTagType("checked")).toBe("warning");
  });
  it("obsolete/未知 为 danger", () => {
    expect(stateTagType("obsolete")).toBe("danger");
    expect(stateTagType("x")).toBe("danger");
  });
});

// ==================== 增量12 RF2：refresh 单飞（终审 I1 补钉） ====================
import { createHttp } from "../src/api/http";

type Cfg = { url?: string; headers: Record<string, string>; _retry?: boolean };

/// 假 adapter：按脚本返回状态码，记录每次请求的 authorization
function mockAdapter(script: Map<string, number[]>, seen: { auth: string[]; urls: string[] }) {
  return async (rawCfg: unknown): Promise<never> => {
    const c = rawCfg as unknown as Cfg;
    if (c.headers && typeof (c.headers as Record<string, unknown>).set !== "function") {
      (c.headers as Record<string, unknown>).set = function (k: string, v: string) {
        (this as Record<string, unknown>)[k] = v;
      };
    }
    const url = c.url ?? "";
    seen.urls.push(url);
    seen.auth.push((c.headers as Record<string, string>)?.["authorization"] ?? "");
    const queue = script.get(url.split("?")[0]) ?? [];
    const status = queue.shift() ?? 200;
    if (status >= 400) {
      // 拦截器走 reject 路径所需错误形态
      const e = new Error(`http ${status}`) as Error & { response?: unknown; config?: unknown };
      e.response = { status };
      e.config = rawCfg;
      throw e;
    }
    return { status, data: { ok: 1 }, statusText: "", headers: {}, config: rawCfg } as never;
  };
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

it("refresh 单飞：并发 401 共享一次换发，重放带新 token", async () => {
  const script = new Map<string, number[]>([["/api/v1/things", [401, 401, 200]]]);
  const seen = { auth: [] as string[], urls: [] as string[] };
  let refreshCalls = 0;
  // 可变 token 闭包——模拟 index.ts 真实行为（onRefresh 更新 localStorage，tokenGetter 读回）
  let current = "old-token";
  const http = createHttp(() => current, {
    adapter: mockAdapter(script, seen),
    onRefresh: async () => {
      refreshCalls += 1;
      await sleep(10); // 模拟网络延迟，制造并发窗口
      current = "new-token";
      return current;
    },
  });
  const [a, b] = await Promise.all([
    http.get("/api/v1/things").then((r) => r.status).catch(() => 0),
    http.get("/api/v1/things").then((r) => r.status).catch(() => 0),
  ]);
  expect(refreshCalls).toBe(1);
  expect([a, b]).toEqual([200, 200]);
  // 重放请求带新 token（最后一次 thing 请求的 authorization）
  const thingAuth = seen.auth.filter((_, i) => seen.urls[i].includes("/api/v1/things"));
  expect(thingAuth.some((h) => h.includes("new-token"))).toBe(true);
  expect(thingAuth.some((h) => h.includes("old-token"))).toBe(true);
});

it("refresh 失败 → onUnauthorized 被调", async () => {
  const script = new Map<string, number[]>([["/api/v1/things", [401]]]);
  const seen = { auth: [] as string[], urls: [] as string[] };
  let unauthorized = 0;
  const http = createHttp(() => "old-token", {
    adapter: mockAdapter(script, seen),
    onRefresh: async () => null,
  });
  const { setUnauthorizedHandler } = await import("../src/api/http");
  setUnauthorizedHandler(() => {
    unauthorized += 1;
  });
  await http.get("/api/v1/things").catch(() => 0);
  expect(unauthorized).toBe(1);
});

it("/auth/ 路由 401 直接跳登录（不触发 refresh）", async () => {
  const script = new Map<string, number[]>([["/api/v1/auth/login", [401]]]);
  const seen = { auth: [] as string[], urls: [] as string[] };
  let refreshCalls = 0;
  let unauthorized = 0;
  const http = createHttp(() => "", {
    adapter: mockAdapter(script, seen),
    onRefresh: async () => {
      refreshCalls += 1;
      return "new-token";
    },
  });
  const { setUnauthorizedHandler } = await import("../src/api/http");
  setUnauthorizedHandler(() => {
    unauthorized += 1;
  });
  await http.post("/api/v1/auth/login", {}).catch(() => 0);
  expect(refreshCalls).toBe(0);
  expect(unauthorized).toBe(1);
});
