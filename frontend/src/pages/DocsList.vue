<script setup lang="ts">
import { ElMessage } from "element-plus";
import { onMounted, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { extractApiError } from "../api/http";
import { http } from "../api/index";
import { listDocs, uploadDoc, type BizObject, type ListParams } from "../api/docs";
import { useAuthStore } from "../stores/auth";
import { stateLabel, stateTagType } from "../utils/flowUi";
import { validateUpload } from "../utils/upload";

const route = useRoute();
const router = useRouter();
const auth = useAuthStore();

const items = ref<BizObject[]>([]);
const total = ref(0);
const page = ref(1);
const size = ref(20);
const material = ref("");
// 看板联动：/docs?state=xxx 初始化状态筛选（增量11）
const state = ref((route.query.state as string) ?? "");
const loading = ref(false);

const uploadVisible = ref(false);
const upObjNo = ref("");
const upName = ref("");
const upMaterial = ref("");
const upFile = ref<File | null>(null);
const uploading = ref(false);

async function load() {
  loading.value = true;
  try {
    const p: ListParams = { page: page.value, size: size.value };
    if (material.value) p.material = material.value;
    if (state.value) p.state = state.value;
    const res = await listDocs(http, p);
    items.value = res.items;
    total.value = res.total;
  } catch (e) {
    ElMessage.error(extractApiError(e));
  } finally {
    loading.value = false;
  }
}

onMounted(load);

function onStateChange() {
  page.value = 1;
  // 增量11 终审 M2 清偿：筛选与 URL 回写（刷新保持）
  router.replace({ query: { ...route.query, state: state.value || undefined } });
  load();
}

function openUpload() {
  uploadVisible.value = true;
}

function onFileChange(f: File | undefined) {
  upFile.value = f ?? null;
}

async function doUpload() {
  if (!upObjNo.value || !upFile.value) {
    ElMessage.warning("需填写图号并选择文件");
    return;
  }
  // 客户端预校验（增量12：增量7 M2 清偿）
  const chk = validateUpload(upFile.value);
  if (!chk.ok) {
    ElMessage.error(chk.reason ?? "文件不可上传");
    return;
  }
  if (chk.warn) ElMessage.warning("非常规图纸格式，已允许继续上传");
  uploading.value = true;
  try {
    await uploadDoc(http, {
      obj_no: upObjNo.value,
      attrs: { name: upName.value || upObjNo.value, material: upMaterial.value },
      file: upFile.value,
    });
    ElMessage.success("上传成功");
    uploadVisible.value = false;
    upObjNo.value = "";
    upName.value = "";
    upMaterial.value = "";
    upFile.value = null;
    await load();
  } catch (e) {
    ElMessage.error(extractApiError(e));
  } finally {
    uploading.value = false;
  }
}

function openDetail(row: BizObject) {
  router.push(`/docs/${row.obj_no}`);
}
</script>

<template>
  <div class="page">
    <div class="toolbar">
      <el-input
        v-model="material"
        placeholder="按材料筛选"
        style="width: 200px"
        clearable
        @change="load"
      />
      <el-select
        v-model="state"
        placeholder="按状态筛选"
        clearable
        style="width: 140px"
        data-test="state-filter"
        @change="onStateChange"
      >
        <el-option label="工作中" value="working" />
        <el-option label="审核中" value="review" />
        <el-option label="待复核" value="checked" />
        <el-option label="已发布" value="released" />
        <el-option label="已作废" value="obsolete" />
      </el-select>
      <el-button v-if="auth.canWrite" type="primary" data-test="open-upload" @click="openUpload">上传图纸</el-button>
      <el-button @click="load">刷新</el-button>
    </div>

    <el-table v-loading="loading" :data="items" @row-click="openDetail">
      <el-table-column prop="obj_no" label="图号" min-width="140" />
      <el-table-column label="名称" min-width="140">
        <template #default="{ row }">{{ row.attrs?.name ?? "-" }}</template>
      </el-table-column>
      <el-table-column label="版本" width="80">
        <template #default="{ row }">v{{ row.major }}.{{ row.minor }}</template>
      </el-table-column>
      <el-table-column label="材料" min-width="100">
        <template #default="{ row }">{{ row.attrs?.material ?? "-" }}</template>
      </el-table-column>
      <el-table-column label="密级" width="80">
        <template #default="{ row }">
          <el-tag
            :type="row.secret_level >= 3 ? 'danger' : row.secret_level === 2 ? 'warning' : 'info'"
            effect="plain"
            size="small"
          >
            {{ row.secret_level }}
          </el-tag>
        </template>
      </el-table-column>
      <el-table-column label="状态" width="100">
        <template #default="{ row }">
          <el-tag :type="stateTagType(row.state)">{{ stateLabel(row.state) }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="更新时间" min-width="160">
        <template #default="{ row }">{{ row.updated_at ?? "-" }}</template>
      </el-table-column>
    </el-table>

    <el-pagination
      v-model:current-page="page"
      v-model:page-size="size"
      layout="total, prev, pager, next"
      :total="total"
      style="margin-top: 12px"
      @current-change="load"
    />

    <el-dialog v-model="uploadVisible" title="上传图纸" width="480px">
      <el-form label-width="80px">
        <el-form-item label="图号">
          <el-input v-model="upObjNo" data-test="upload-objno" placeholder="DWG-001" />
        </el-form-item>
        <el-form-item label="名称">
          <el-input v-model="upName" data-test="upload-name" placeholder="支架" />
        </el-form-item>
        <el-form-item label="材料">
          <el-input v-model="upMaterial" data-test="upload-material" placeholder="Q235" />
        </el-form-item>
        <el-form-item label="文件">
          <input type="file" data-test="upload-file" @change="onFileChange(($event.target as HTMLInputElement).files?.[0])" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="uploadVisible = false">取消</el-button>
        <el-button type="primary" data-test="upload-submit" :loading="uploading" @click="doUpload">
          上传
        </el-button>
      </template>
    </el-dialog>
  </div>
</template>
