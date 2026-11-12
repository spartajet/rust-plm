/// 生命周期 UI 语义（纯函数，可穷举单测——计划 Review Focus #2）。
/// 返回各操作按钮是否对当前状态可见/可用。
export interface FlowUi {
  submit: boolean;
  approve: boolean;
  reject: boolean;
  obsolete: boolean;
  /// 升版（仅 released；增量8）
  revise: boolean;
}

const ALL_OFF: FlowUi = { submit: false, approve: false, reject: false, obsolete: false, revise: false };

export function flowUi(state: string): FlowUi {
  switch (state) {
    case "working":
      return { ...ALL_OFF, submit: true, obsolete: true };
    case "review":
      return { ...ALL_OFF, approve: true, reject: true };
    case "checked":
      // strict 档第二级复核（增量11 M2 清偿）：同 approve/reject 按钮组驱动
      return { ...ALL_OFF, approve: true, reject: true };
    case "released":
      return { ...ALL_OFF, obsolete: true, revise: true };
    default:
      return { ...ALL_OFF };
  }
}

export const STATE_LABEL: Record<string, string> = {
  working: "工作中",
  review: "审核中",
  checked: "待复核",
  released: "已发布",
  obsolete: "已作废",
};

export function stateLabel(state: string): string {
  return STATE_LABEL[state] ?? state;
}

export function stateTagType(state: string): "info" | "warning" | "success" | "danger" {
  switch (state) {
    case "working":
      return "info";
    case "review":
      return "warning";
    case "released":
      return "success";
    default:
      return "danger";
  }
}
