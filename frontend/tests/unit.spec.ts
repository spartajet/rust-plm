import { describe, expect, it } from "vitest";
import { flowUi, stateLabel } from "../src/utils/flowUi";
import { guardTarget } from "../src/router/index";
import { buildListQuery, parseNdjson } from "../src/api/docs";

// Review Focus #2：flowUi 全状态穷举
describe("flowUi", () => {
  it("working: submit+obsolete only", () => {
    expect(flowUi("working")).toEqual({ submit: true, approve: false, reject: false, obsolete: true });
  });
  it("review: approve+reject only", () => {
    expect(flowUi("review")).toEqual({ submit: false, approve: true, reject: true, obsolete: false });
  });
  it("released: obsolete only", () => {
    expect(flowUi("released")).toEqual({ submit: false, approve: false, reject: false, obsolete: true });
  });
  it("obsolete: all off", () => {
    expect(flowUi("obsolete")).toEqual({ submit: false, approve: false, reject: false, obsolete: false });
  });
  it("unknown state: all off", () => {
    expect(flowUi("nonsense")).toEqual({ submit: false, approve: false, reject: false, obsolete: false });
  });
  it("labels", () => {
    expect(stateLabel("working")).toBe("工作中");
    expect(stateLabel("released")).toBe("已发布");
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
