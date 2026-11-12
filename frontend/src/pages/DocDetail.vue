<script setup lang="ts">
import { ElMessage } from "element-plus";
import { computed, onMounted, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { extractApiError } from "../api/http";
import { http } from "../api/index";
import { bomExpand, getDoc, listVersions, transition, type BomRow, type DocResponse, type VersionSnapshot } from "../api/docs";
import { flowUi, stateLabel, stateTagType } from "../utils/flowUi";
import type { LifecycleAction } from "../api/docs";

const route = useRoute();
const router = useRouter();
const objNo = route.params.objNo as string;

const doc = ref<DocResponse | null>(null);
const bomRows = ref<BomRow[]>([]);
const snapshots = ref<VersionSnapshot[]>([]);
const loading = ref(false);
const acting = ref(false);

const ui = computed(() => flowUi(doc.value?.object.state ?? ""));

async function load() {
  loading.value = true;
  try {
    doc.value = await getDoc(http, objNo);
    bomRows.value = await bomExpand(http, objNo, "onelevel");
    snapshots.value = (await listVersions(http, objNo)).snapshots;
  } catch (e) {
    ElMessage.error(extractApiError(e));
  } finally {
    loading.value = false;
  }
}

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

function download() {
  window.open(
    `${import.meta.env.VITE_API_BASE ?? "http://localhost:8080"}/api/v1/docs/${objNo}/file`,
    "_blank",
  );
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
        <el-button v-if="ui.submit" type="primary" data-test="act-submit" :loading="acting" @click="act('submit')">
          提交评审
        </el-button>
        <el-button v-if="ui.approve" type="success" data-test="act-approve" :loading="acting" @click="act('approve')">
          审批通过
        </el-button>
        <el-button v-if="ui.reject" type="warning" data-test="act-reject" :loading="acting" @click="act('reject')">
          驳回修改
        </el-button>
        <el-button v-if="ui.obsolete" type="danger" data-test="act-obsolete" :loading="acting" @click="act('obsolete')">
          作废
        </el-button>
        <el-button v-if="ui.revise" type="primary" plain data-test="act-revise" :loading="acting" @click="act('revise')">
          升新版
        </el-button>
        <el-button v-if="doc.file" @click="download">下载文件（{{ doc.file.name }}）</el-button>
      </div>
    </el-card>

    <el-card style="margin-top: 16px">
      <template #header>BOM（一级明细）</template>
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
