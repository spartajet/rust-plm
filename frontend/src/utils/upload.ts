/// 上传客户端预校验（增量12：增量7 M2 清偿——纯函数可单测）
/// 大小硬上限与后端缺省 max_upload(200MB) 一致；类型白名单外仅 WARN（图纸长尾格式开放——台账裁定）

export const MAX_UPLOAD_MB = 200;
const ALLOWED_EXT = [
  "dwg", "dxf", "step", "stp", "iges", "igs", "pdf",
  "png", "jpg", "jpeg", "zip", "xls", "xlsx",
];

export function validateUpload(file: File): { ok: boolean; warn?: boolean; reason?: string } {
  if (file.size <= 0) return { ok: false, reason: "文件为空" };
  if (file.size > MAX_UPLOAD_MB * 1024 * 1024)
    return { ok: false, reason: `超过 ${MAX_UPLOAD_MB}MB 上限` };
  const ext = file.name.includes(".") ? file.name.split(".").pop()!.toLowerCase() : "";
  if (!ext || !ALLOWED_EXT.includes(ext)) return { ok: true, warn: true };
  return { ok: true };
}
