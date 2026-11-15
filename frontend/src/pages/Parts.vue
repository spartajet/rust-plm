<script setup lang="ts">
// 物料列表页（增量17 PART 纵切）：分类树创建 + 查重提示 + 列表
import { ElMessage, ElMessageBox } from "element-plus";
import { onMounted, ref } from "vue";
import { extractApiError } from "../api/http";
import { http } from "../api/index";
import {
  classTree,
  createPart,
  duplicates,
  listParts,
  type ClassNode,
  type Duplicate,
  type PartRow,
} from "../api/part";

const items = ref<PartRow[]>([]);
const total = ref(0);
const page = ref(1);
const size = ref(20);
const loading = ref(false);

const tree = ref<ClassNode[]>([]);
const dlgVisible = ref(false);
const clsCode = ref("");
const pName = ref("");
const pMaterial = ref("");
const pSpec = ref("");
const submitting = ref(false);

async function load() {
  loading.value = true;
  try {
    const res = await listParts(http, page.value, size.value);
    items.value = res.items;
    total.value = res.total;
  } catch (e) {
    ElMessage.error(extractApiError(e));
  } finally {
    loading.value = false;
  }
}

async function loadTree() {
  try {
    tree.value = await classTree(http);
  } catch (e) {
    ElMessage.error(extractApiError(e));
  }
}

onMounted(() => {
  load();
  loadTree();
});

/** 扁平化分类树供下拉选择（含父子路径名） */
const classOptions = ref<{ code: string; label: string }[]>([]);
function flatten(nodes: ClassNode[], prefix: string) {
  for (const n of nodes) {
    const label = prefix ? `${prefix} / ${n.name}` : n.name;
    if (n.has_rule) classOptions.value.push({ code: n.code, label: `${label}（${n.code}）` });
    flatten(n.children, label);
  }
}

function openCreate() {
  classOptions.value = [];
  flatten(tree.value, "");
  clsCode.value = "";
  pName.value = "";
  pMaterial.value = "";
  pSpec.value = "";
  dlgVisible.value = true;
}

async function submit() {
  if (!clsCode.value || !pName.value.trim()) {
    ElMessage.warning("需选择分类并填写名称");
    return;
  }
  submitting.value = true;
  try {
    // 先查重提示（PART-05 MVP：不阻断）
    let dup: Duplicate[] = [];
    try {
      dup = await duplicates(http, pName.value, pMaterial.value);
    } catch {
      /* 查重失败不阻断创建 */
    }
    if (dup.length > 0) {
      await ElMessageBox.confirm(
        `发现 ${dup.length} 个相似件（${dup.map((d) => d.obj_no).join("、")}），仍要创建吗？`,
        "查重提示",
        { confirmButtonText: "仍要创建", cancelButtonText: "取消", type: "warning" },
      );
    }
    const res = await createPart(http, {
      class_code: clsCode.value,
      attrs: {
        name: pName.value,
        material: pMaterial.value || null,
        spec: pSpec.value || null,
      },
    });
    ElMessage.success(`已创建 ${res.obj_no}`);
    dlgVisible.value = false;
    await load();
  } catch (e) {
    if ((e as { message?: string }).message !== "cancel") {
      ElMessage.error(extractApiError(e));
    }
  } finally {
    submitting.value = false;
  }
}
</script>

<template>
  <div class="page">
    <div class="toolbar">
      <el-button type="primary" data-test="open-create-part" @click="openCreate">新建物料</el-button>
      <el-button data-test="reload-parts" @click="load">刷新</el-button>
    </div>

    <el-table v-loading="loading" :data="items" data-test="parts-table">
      <el-table-column prop="obj_no" label="物料号" min-width="180" />
      <el-table-column label="名称" min-width="140">
        <template #default="{ row }">{{ row.attrs?.name ?? "-" }}</template>
      </el-table-column>
      <el-table-column label="分类" min-width="120">
        <template #default="{ row }">{{ row.attrs?.class_name ?? "-" }}</template>
      </el-table-column>
      <el-table-column label="材料" min-width="100">
        <template #default="{ row }">{{ row.attrs?.material ?? "-" }}</template>
      </el-table-column>
      <el-table-column label="规格" min-width="100">
        <template #default="{ row }">{{ row.attrs?.spec ?? "-" }}</template>
      </el-table-column>
      <el-table-column label="版本" width="80">
        <template #default="{ row }">v{{ row.major }}.{{ row.minor }}</template>
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

    <el-dialog v-model="dlgVisible" title="新建物料" width="480px">
      <el-form label-width="90px">
        <el-form-item label="分类">
          <el-select v-model="clsCode" data-test="part-class" placeholder="选择分类（编码规则挂分类）" style="width: 100%">
            <el-option v-for="o in classOptions" :key="o.code" :label="o.label" :value="o.code" />
          </el-select>
        </el-form-item>
        <el-form-item label="名称">
          <el-input v-model="pName" data-test="part-name" />
        </el-form-item>
        <el-form-item label="材料">
          <el-input v-model="pMaterial" data-test="part-material" placeholder="如 Q235 / 304" />
        </el-form-item>
        <el-form-item label="规格">
          <el-input v-model="pSpec" data-test="part-spec" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="dlgVisible = false">取消</el-button>
        <el-button type="primary" data-test="part-submit" :loading="submitting" @click="submit">
          创建（自动赋码）
        </el-button>
      </template>
    </el-dialog>
  </div>
</template>
