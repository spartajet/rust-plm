<script setup lang="ts">
import { ElMessage } from "element-plus";
import { computed, onMounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { extractApiError } from "../api/http";
import { http } from "../api/index";
import { bomExpand, downloadFile, getDoc, listVersions, whereUsedAsof, whereUsedLive, transition, type BomRow, type DocResponse, type VersionSnapshot, type WhereUsedAsofRow, type WhereUsedLiveRow } from "../api/docs";
import { useAuthStore } from "../stores/auth";
import { flowUi, stateLabel, stateTagType } from "../utils/flowUi";
import type { LifecycleAction } from "../api/docs";

const route = useRoute();
const router = useRouter();
const objNo = route.params.objNo as string;

const doc = ref<DocResponse | null>(null);
const bomRows = ref<BomRow[]>([]);
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
    bomRows.value = await bomExpand(http, objNo, "onelevel", bomMajor.value || undefined);
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
  bomRows.value = []; // 切换失败时不得残留上一版行（终审 M4）
  try {
    bomRows.value = await bomExpand(http, objNo, "onelevel", bomMajor.value || undefined);
  } catch (e) {
    ElMessage.error(extractApiError(e));
  }
}

watch(bomMajor, reloadBom);

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
      <el-table :data="bomRows" size="small">
        <el-table-column label="子件" min-width="280">
          <template #default="{ row }">{{ row.child }}</template>
        </el-table-column>
        <el-table-column label="数量" width="120">
          <template #default="{ row }">{{ row.qty_total }}</template>
        </el-table-column>
      </el-table>
      <div v-if="bomRows.length === 0" style="color: #909399">无 BOM 行</div>
    </el-card>

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
