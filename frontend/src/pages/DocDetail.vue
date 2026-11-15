<script setup lang="ts">
import { ElMessage } from "element-plus";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { extractApiError, extractBlobApiError } from "../api/http";
import { http } from "../api/index";
import {
  previewDoc,
  bomExpand,
  bomLines,
  bomSummary,
  buildSetLinesPayload,
  downloadBomSummaryCsv,
  downloadFile,
  getDoc,
  listVersions,
  setBomLines,
  whereUsedAsof,
  whereUsedLive,
  transition,
  type BomEditRow,
  type BomRow,
  type BomSummaryRow,
  type DocResponse,
  type VersionSnapshot,
  type WhereUsedAsofRow,
  type WhereUsedLiveRow,
} from "../api/docs";
import { searchGlobal } from "../api/search";
import { useAuthStore } from "../stores/auth";
import { flowUi, stateLabel, stateTagType } from "../utils/flowUi";
import type { LifecycleAction } from "../api/docs";

const route = useRoute();
const router = useRouter();
const objNo = route.params.objNo as string;

const doc = ref<DocResponse | null>(null);
const bomRows = ref<BomRow[]>([]);
// 富行（增量21）：编号/名称直接可读；live 与 as-of 均走 /lines
const bomLineInfos = ref<Awaited<ReturnType<typeof bomLines>>>([]);
const snapshots = ref<VersionSnapshot[]>([]);
const wuRows = ref<WhereUsedAsofRow[]>([]);
const wuLive = ref<WhereUsedLiveRow[]>([]);
const loading = ref(false);
const acting = ref(false);
// BOM 版本切换（增量11）：0=live；N=发布快照 major=N as-of
const bomMajor = ref(0);

const auth = useAuthStore();
const ui = computed(() => flowUi(doc.value?.object.state ?? ""));
const bomOptions = computed(() => [
  { label: "当前版本", value: 0 },
  ...snapshots.value.map((s) => ({
    label: `v${s.major}.${s.minor} 发布快照`,
    value: s.major,
  })),
]);

async function load() {
  loading.value = true;
  try {
    doc.value = await getDoc(http, objNo);
    bomLineInfos.value = await bomLines(http, objNo, bomMajor.value || undefined);
    snapshots.value = (await listVersions(http, objNo)).snapshots;
  } catch (e) {
    ElMessage.error(extractApiError(e));
    loading.value = false;
    return;
  }
  // 被谁使用=非关键卡：失败独立降级空态（终审 M4），不拖垮详情主视图
  try {
    wuRows.value = await whereUsedAsof(http, objNo);
  } catch {
    wuRows.value = [];
  }
  try {
    wuLive.value = await whereUsedLive(http, objNo);
  } catch {
    wuLive.value = [];
  }
  loading.value = false;
}

async function reloadBom() {
  bomLineInfos.value = []; // 切换失败时不得残留上一版行（终审 M4）
  try {
    bomLineInfos.value = await bomLines(http, objNo, bomMajor.value || undefined);
  } catch (e) {
    ElMessage.error(extractApiError(e));
  }
}

// ==================== 增量21 BOM-01 编辑器 + BOM-07 汇总 ====================
const editing = ref(false);
const editRows = ref<BomEditRow[]>([]);
const saving = ref(false);
const addVisible = ref(false);
const addQ = ref("");
const addHits = ref<{ obj_id: string; obj_no: string; name?: string | null; type_code: string }[]>([]);
const addSearching = ref(false);
// 汇总折叠（懒加载）
const bomSummaryRows = ref<BomSummaryRow[]>([]);
const bomSummaryLoaded = ref(false);
// 折叠展开态（终审 M1-3：major 切换时若面板开着须自动刷新，避免误导性空态）
const summaryOpen = ref<string[]>([]);

// ==================== DOC-05 在线预览（增量22） ====================
const previewState = ref<{ url: string; kind: "pdf" | "image" } | null>(null);
const previewLoading = ref(false);
// 水印（overlay 起步：用户+密级+分钟级时间；服务端内嵌水印后置——台账裁定）
const watermarkText = computed(
  () => `${auth.username} · 密级${doc.value?.object.secret_level ?? 0} · ${new Date().toISOString().slice(0, 16)}`,
);

// 终审 M1：previewDoc 在途时导航离开——onBeforeUnmount 先行（previewState 尚空），
// promise 兑现后 createObjectURL 无人释放；disposed 闸门拦截
let previewDisposed = false;

async function doPreview() {
  previewLoading.value = true;
  try {
    let result: { blob: Blob; mime: string };
    try {
      result = await previewDoc(http, objNo);
    } catch (e) {
      // 415 等错误体为 blob——异步解出后端 message（"该格式不支持在线预览…"）
      ElMessage.error(await extractBlobApiError(e));
      return;
    }
    if (previewDisposed) return;
    const { blob, mime } = result;
    const kind = mime === "application/pdf" ? "pdf" : "image";
    const url = URL.createObjectURL(blob);
    if (previewState.value) URL.revokeObjectURL(previewState.value.url);
    previewState.value = { url, kind };
  } finally {
    previewLoading.value = false;
  }
}

function closePreview() {
  if (previewState.value) URL.revokeObjectURL(previewState.value.url);
  previewState.value = null;
}

onBeforeUnmount(() => {
  previewDisposed = true;
  if (previewState.value) URL.revokeObjectURL(previewState.value.url);
});

const bomEditable = computed(() => auth.canWrite && bomMajor.value === 0);

function startEdit() {
  editRows.value = (bomLineInfos.value ?? []).map((r) => ({
    child_id: r.child_id,
    child_obj_no: r.child_obj_no,
    child_name: r.child_name,
    qty: r.qty,
  }));
  editing.value = true;
}

function cancelEdit() {
  editing.value = false;
  editRows.value = [];
}

async function saveEdit() {
  if (editRows.value.some((r) => r.qty === null || r.qty <= 0)) {
    ElMessage.warning("数量须为正数");
    return;
  }
  const ids = new Set(editRows.value.map((r) => r.child_id));
  if (ids.size !== editRows.value.length) {
    ElMessage.warning("存在重复子件行");
    return;
  }
  saving.value = true;
  try {
    await setBomLines(http, objNo, buildSetLinesPayload(editRows.value));
    ElMessage.success("BOM 已保存");
    editing.value = false;
    editRows.value = [];
    await reloadBom();
  } catch (e) {
    // 409 循环引用/400 校验错误原样透出
    ElMessage.error(extractApiError(e));
  } finally {
    saving.value = false;
  }
}

function removeRow(i: number) {
  editRows.value.splice(i, 1);
}

async function searchForAdd() {
  const q = addQ.value.trim();
  if (!q) return;
  addSearching.value = true;
  try {
    const res = await searchGlobal(http, q);
    addHits.value = res.groups.flatMap((g) =>
      g.items.map((i) => ({
        obj_id: i.obj_id ?? "",
        obj_no: i.obj_no,
        name: i.name,
        type_code: g.type_code,
      })),
    );
  } catch (e) {
    ElMessage.error(extractApiError(e));
  } finally {
    addSearching.value = false;
  }
}

function pickHit(h: { obj_id: string; obj_no: string; name?: string | null }) {
  if (!h.obj_id) return;
  if (h.obj_id === (doc.value?.object.id ?? "")) {
    ElMessage.warning("不能引用自身（循环）");
    return;
  }
  if (editRows.value.some((r) => r.child_id === h.obj_id)) {
    ElMessage.warning("该子件已在行中");
    return;
  }
  editRows.value.push({ child_id: h.obj_id, child_obj_no: h.obj_no, child_name: h.name, qty: 1 });
  addVisible.value = false;
  addQ.value = "";
  addHits.value = [];
}

async function loadSummary() {
  try {
    bomSummaryRows.value = await bomSummary(http, objNo, bomMajor.value || undefined);
    bomSummaryLoaded.value = true;
  } catch (e) {
    ElMessage.error(extractApiError(e));
  }
}

async function exportCsv() {
  try {
    const blob = await downloadBomSummaryCsv(http, objNo, bomMajor.value || undefined);
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `summary-${objNo}.csv`;
    a.click();
    URL.revokeObjectURL(url);
  } catch (e) {
    ElMessage.error(extractApiError(e));
  }
}

watch(bomMajor, () => {
  // 版本切换：编辑态失效 + 汇总缓存作废 + 重载行（原增量11 语义保留）
  editing.value = false;
  editRows.value = [];
  bomSummaryLoaded.value = false;
  bomSummaryRows.value = [];
  reloadBom();
  // 面板处于展开态 → 直接刷新（否则空态误导至用户重新折叠展开）
  if (summaryOpen.value.includes("summary")) loadSummary();
});

onMounted(load);

async function act(action: LifecycleAction) {
  acting.value = true;
  try {
    await transition(http, objNo, action);
    ElMessage.success("操作成功");
    await load();
  } catch (e) {
    ElMessage.error(extractApiError(e));
  } finally {
    acting.value = false;
  }
}

async function download() {
  try {
    const blob = await downloadFile(http, objNo);
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = doc.value?.file?.name ?? objNo;
    a.click();
    URL.revokeObjectURL(url);
  } catch (e) {
    ElMessage.error(extractApiError(e));
  }
}

function back() {
  router.push("/docs");
}
</script>

<template>
  <div class="page" v-loading="loading">
    <el-page-header @back="back">
      <template #content>
        <span style="font-weight: 600">{{ objNo }}</span>
        <el-tag
          v-if="doc"
          :type="stateTagType(doc.object.state)"
          style="margin-left: 12px"
          data-test="state-tag"
        >
          {{ stateLabel(doc.object.state) }}
        </el-tag>
        <el-tag v-if="doc" type="info" effect="plain" style="margin-left: 8px" data-test="ver-badge">
          v{{ doc.object.major }}.{{ doc.object.minor }}
        </el-tag>
      </template>
    </el-page-header>

    <el-card v-if="doc" style="margin-top: 16px">
      <el-descriptions :column="3" border>
        <el-descriptions-item label="名称">
          {{ doc.object.attrs?.name ?? "-" }}
        </el-descriptions-item>
        <el-descriptions-item label="材料">
          {{ doc.object.attrs?.material ?? "-" }}
        </el-descriptions-item>
        <el-descriptions-item label="密级">
          {{ doc.object.secret_level }}
        </el-descriptions-item>
      </el-descriptions>

      <div style="margin-top: 16px; display: flex; gap: 8px">
        <el-button v-if="ui.submit && auth.canWrite" type="primary" data-test="act-submit" :loading="acting" @click="act('submit')">
          提交评审
        </el-button>
        <el-button v-if="ui.approve && auth.canApprove" type="success" data-test="act-approve" :loading="acting" @click="act('approve')">
          审批通过
        </el-button>
        <el-button v-if="ui.reject && auth.canApprove" type="warning" data-test="act-reject" :loading="acting" @click="act('reject')">
          驳回修改
        </el-button>
        <el-button v-if="ui.obsolete && auth.canApprove" type="danger" data-test="act-obsolete" :loading="acting" @click="act('obsolete')">
          作废
        </el-button>
        <el-button v-if="ui.revise && auth.canWrite" type="primary" plain data-test="act-revise" :loading="acting" @click="act('revise')">
          升新版
        </el-button>
        <el-button v-if="doc.file" @click="download">下载文件（{{ doc.file.name }}）</el-button>
        <el-button
          v-if="doc.file"
          type="primary"
          plain
          data-test="preview-btn"
          :loading="previewLoading"
          @click="doPreview"
        >
          在线预览
        </el-button>
      </div>

      <!-- DOC-05 预览区（blob 直开 + 平铺水印 overlay） -->
      <div v-if="previewState" class="preview-wrap" data-test="preview-area">
        <img
          v-if="previewState.kind === 'image'"
          :src="previewState.url"
          style="max-width: 100%; display: block"
          data-test="preview-img"
        />
        <!-- 终审 I-1 备案：不加 sandbox——内容经后端 MIME 白名单（仅 pdf 到 iframe）+nosniff
             双闸，无同源脚本执行路径；加 sandbox 会破坏浏览器 PDF 查看器（裁定互链 migrations.md） -->
        <iframe
          v-else
          :src="previewState.url"
          style="width: 100%; height: 520px; border: 0"
          data-test="preview-frame"
        />
        <div class="watermark" data-test="preview-watermark">
          <span v-for="i in 24" :key="i" class="wm-item">{{ watermarkText }}</span>
        </div>
        <el-button
          link
          style="position: absolute; top: 4px; right: 8px"
          data-test="preview-close"
          @click="closePreview"
        >
          关闭预览
        </el-button>
      </div>
    </el-card>

    <el-card style="margin-top: 16px" data-test="bom-card">
      <template #header>
        <div style="display: flex; align-items: center; gap: 12px">
          <span>BOM（一级明细）</span>
          <el-select
            v-model="bomMajor"
            size="small"
            style="width: 180px"
            data-test="bom-ver-select"
          >
            <el-option
              v-for="o in bomOptions"
              :key="o.value"
              :label="o.label"
              :value="o.value"
            />
          </el-select>
        </div>
      </template>
      <div v-if="bomEditable" style="margin-bottom: 8px">
        <el-button v-if="!editing" size="small" type="primary" data-test="bom-edit-btn" @click="startEdit">
          编辑结构
        </el-button>
        <template v-else>
          <el-button size="small" type="success" data-test="bom-save-btn" :loading="saving" @click="saveEdit">
            保存
          </el-button>
          <el-button size="small" data-test="bom-cancel-btn" @click="cancelEdit">取消</el-button>
          <el-button size="small" type="primary" plain data-test="bom-add-btn" @click="addVisible = true">
            添加子件
          </el-button>
        </template>
      </div>
      <div v-if="bomMajor !== 0" style="color: #909399; margin-bottom: 8px" data-test="bom-readonly-hint">
        发布快照只读（切回当前版本可编辑）
      </div>

      <!-- 展示态：富行（编号/名称可读） -->
      <el-table v-if="!editing" :data="bomLineInfos" size="small" data-test="bom-lines-table">
        <el-table-column label="行号" width="70">
          <template #default="{ row }">{{ row.line_no || "-" }}</template>
        </el-table-column>
        <el-table-column label="子件编号" min-width="150">
          <template #default="{ row }">{{ row.child_obj_no }}</template>
        </el-table-column>
        <el-table-column label="子件名称" min-width="150">
          <template #default="{ row }">{{ row.child_name ?? "-" }}</template>
        </el-table-column>
        <el-table-column label="数量" width="100">
          <template #default="{ row }">{{ row.qty }}</template>
        </el-table-column>
      </el-table>
      <!-- 编辑态：qty 行内编辑 + 删行 -->
      <el-table v-else :data="editRows" size="small" data-test="bom-edit-table">
        <el-table-column label="行号" width="70">
          <template #default="{ $index }">{{ $index + 1 }}</template>
        </el-table-column>
        <el-table-column label="子件编号" min-width="150">
          <template #default="{ row }">{{ row.child_obj_no }}</template>
        </el-table-column>
        <el-table-column label="子件名称" min-width="140">
          <template #default="{ row }">{{ row.child_name ?? "-" }}</template>
        </el-table-column>
        <el-table-column label="数量" width="130">
          <template #default="{ row }">
            <el-input-number v-model="row.qty" :min="1" size="small" data-test="bom-qty-input" />
          </template>
        </el-table-column>
        <el-table-column label="" width="80">
          <template #default="{ $index }">
            <el-button link type="danger" size="small" data-test="bom-del-btn" @click="removeRow($index)">
              删除
            </el-button>
          </template>
        </el-table-column>
      </el-table>
      <div v-if="!editing && bomLineInfos.length === 0" style="color: #909399">无 BOM 行</div>

      <!-- BOM-07 汇总折叠（懒加载） -->
      <el-collapse v-model="summaryOpen" style="margin-top: 12px" @change="(names: string | string[]) => (Array.isArray(names) ? names : [names]).includes('summary') && loadSummary()">
        <el-collapse-item title="用量/分类汇总（BOM-07）" name="summary">
          <div style="margin-bottom: 8px">
            <el-button size="small" data-test="bom-csv-btn" @click="exportCsv">导出 CSV</el-button>
          </div>
          <el-table :data="bomSummaryRows" size="small" data-test="bom-summary-table">
            <el-table-column label="编号" min-width="140" prop="obj_no" />
            <el-table-column label="名称" min-width="140">
              <template #default="{ row }">{{ row.name ?? "-" }}</template>
            </el-table-column>
            <el-table-column label="分类" min-width="110">
              <template #default="{ row }">{{ row.class_name ?? "-" }}</template>
            </el-table-column>
            <el-table-column label="单台用量" width="100" prop="qty_total" />
          </el-table>
          <div v-if="bomSummaryRows.length === 0" style="color: #909399">无叶子件</div>
        </el-collapse-item>
      </el-collapse>
    </el-card>

    <!-- 添加子件对话框：全局搜索选件（obj_id 直取） -->
    <el-dialog v-model="addVisible" title="添加子件（全局搜索）" width="560px">
      <div style="display: flex; gap: 8px">
        <el-input
          v-model="addQ"
          placeholder="编号 / 名称关键词"
          data-test="bom-add-search"
          clearable
          @keyup.enter="searchForAdd"
        />
        <el-button data-test="bom-add-search-btn" @click="searchForAdd">搜索</el-button>
      </div>
      <el-table
        :data="addHits"
        size="small"
        style="margin-top: 8px"
        max-height="280"
        data-test="bom-add-results"
        v-loading="addSearching"
      >
        <el-table-column label="编号" min-width="140" prop="obj_no" />
        <el-table-column label="名称" min-width="130">
          <template #default="{ row }">{{ row.name ?? "-" }}</template>
        </el-table-column>
        <el-table-column label="类型" width="90">
          <template #default="{ row }">{{ row.type_code === "part" ? "物料" : "图纸" }}</template>
        </el-table-column>
        <el-table-column label="" width="80">
          <template #default="{ row }">
            <el-button link type="primary" size="small" data-test="bom-add-select" @click="pickHit(row)">
              选择
            </el-button>
          </template>
        </el-table-column>
      </el-table>
    </el-dialog>

    <el-card style="margin-top: 16px">
      <template #header>被谁使用</template>
      <el-divider content-position="left">当前被用（live）</el-divider>
      <el-table :data="wuLive" size="small" data-test="wu-live-table">
        <el-table-column label="用于 BOM" min-width="160">
          <template #default="{ row }">{{ row.ancestor_no }}</template>
        </el-table-column>
        <el-table-column label="数量" width="90">
          <template #default="{ row }">{{ row.qty_total }}</template>
        </el-table-column>
      </el-table>
      <div v-if="wuLive.length === 0" style="color: #909399">当前未被任何 BOM 使用</div>
      <el-divider content-position="left">历史发布快照（as-of）</el-divider>
      <el-table :data="wuRows" size="small" data-test="wu-asof-table">
        <el-table-column label="用于 BOM" min-width="160">
          <template #default="{ row }">{{ row.parent_no }}</template>
        </el-table-column>
        <el-table-column label="发布版本" width="110">
          <template #default="{ row }">v{{ row.major }}.0</template>
        </el-table-column>
        <el-table-column label="所属发布" min-width="160">
          <template #default="{ row }">{{ row.release_root_no }}</template>
        </el-table-column>
        <el-table-column label="数量" width="90">
          <template #default="{ row }">{{ row.qty_total }}</template>
        </el-table-column>
      </el-table>
      <div v-if="wuRows.length === 0" style="color: #909399">尚未被任何已发布 BOM 使用</div>
    </el-card>

    <el-card style="margin-top: 16px">
      <template #header>版本历史（发布快照）</template>
      <el-table :data="snapshots" size="small">
        <el-table-column label="版本" width="90">
          <template #default="{ row }">v{{ row.major }}.{{ row.minor }}</template>
        </el-table-column>
        <el-table-column label="状态" width="110">
          <template #default="{ row }">{{ stateLabel(row.state) }}</template>
        </el-table-column>
        <el-table-column label="发布件哈希" min-width="200">
          <template #default="{ row }">{{ row.file_hash ?? "-" }}</template>
        </el-table-column>
        <el-table-column label="时间" min-width="160">
          <template #default="{ row }">{{ row.created_at ?? "-" }}</template>
        </el-table-column>
      </el-table>
      <div v-if="snapshots.length === 0" style="color: #909399">尚无发布记录</div>
    </el-card>
  </div>
</template>

<style scoped>
/* 增量22 终审 I1 清偿：水印 overlay 真实落样式（T2 时 python replace 静默 no-op——
   原文件无 <style scoped> 块可锚定；此后新增样式一律用 Edit 直写） */
.preview-wrap {
  position: relative;
  margin-top: 12px;
  border: 1px solid #e4e7ed;
  border-radius: 4px;
  padding: 8px;
  overflow: hidden;
}
.watermark {
  position: absolute;
  inset: 0;
  pointer-events: none;
  overflow: hidden;
  z-index: 1;
  display: flex;
  flex-wrap: wrap;
  align-content: space-around;
  justify-content: space-around;
}
.wm-item {
  display: inline-block;
  margin: 18px;
  opacity: 0.12;
  transform: rotate(-18deg);
  font-size: 14px;
  white-space: nowrap;
}
</style>
