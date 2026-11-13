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
