<script setup lang="ts">
import { ElMessage, ElMessageBox } from "element-plus";
import { onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { extractApiError } from "../api/http";
import { http } from "../api/index";
import { listTodos, batchApprove, summarizeBatch, type TodoItem, type TodoBox } from "../api/todos";
import { useAuthStore } from "../stores/auth";
import { stateLabel, stateTagType } from "../utils/flowUi";

const router = useRouter();
const auth = useAuthStore();

const box = ref<TodoBox>("todo");
const items = ref<TodoItem[]>([]);
const total = ref(0);
const page = ref(1);
const size = ref(20);
const loading = ref(false);
const selected = ref<TodoItem[]>([]);
const batchRunning = ref(false);

const BOX_LABEL: Record<TodoBox, string> = { todo: "待办", done: "已办", mine: "我发起" };

async function load() {
  loading.value = true;
  try {
    const res = await listTodos(http, box.value, page.value, size.value);
    items.value = res.items;
    total.value = res.total;
  } catch (e) {
    ElMessage.error(extractApiError(e));
  } finally {
    loading.value = false;
  }
}

onMounted(load);
watch(box, () => {
  page.value = 1;
  selected.value = [];
  load();
});

function switchBox(b: TodoBox) {
  box.value = b;
}

function openDetail(row: TodoItem) {
  router.push(`/docs/${row.obj_no}`);
}

function onSelectionChange(rows: TodoItem[]) {
  selected.value = rows;
}

/// 批量审批（WF-03：同流程多对象；意见经弹窗收集，失败项逐条提示）
async function doBatchApprove() {
  if (!selected.value.length) return;
  try {
    const { value } = await ElMessageBox.prompt(
      `对选中的 ${selected.value.length} 件执行批准，可填写审批意见`,
      "批量审批",
      {
        confirmButtonText: "批准",
        cancelButtonText: "取消",
        inputType: "textarea",
        inputPlaceholder: "审批意见（选填）",
      },
    );
    batchRunning.value = true;
    const res = await batchApprove(http, selected.value.map((r) => r.obj_no), value || undefined);
    const sum = summarizeBatch(res.items);
    if (sum.okCount) ElMessage.success(`已批准 ${sum.okCount} 件`);
    for (const f of sum.failures) {
      ElMessage.error(`${f.obj_no}: ${f.error ?? "失败"}`);
    }
    selected.value = [];
    await load();
  } catch (e) {
    if (e !== "cancel" && e !== "close") ElMessage.error(extractApiError(e));
  } finally {
    batchRunning.value = false;
  }
}
</script>

<template>
  <div class="page">
    <el-tabs v-model="box" data-test="todo-tabs">
      <el-tab-pane
        v-for="b in (['todo', 'done', 'mine'] as TodoBox[])"
        :key="b"
        :name="b"
      >
        <template #label>
          <span :data-test="`tab-${b}`" @click="switchBox(b)">{{ BOX_LABEL[b] }}</span>
        </template>
      </el-tab-pane>
    </el-tabs>

    <div class="toolbar" v-if="box === 'todo' && auth.canApprove">
      <el-button
        type="success"
        data-test="batch-approve"
        :disabled="!selected.length"
        :loading="batchRunning"
        @click="doBatchApprove"
      >
        批量批准{{ selected.length ? `（${selected.length}）` : "" }}
      </el-button>
      <el-button data-test="refresh" @click="load">刷新</el-button>
    </div>
    <div class="toolbar" v-else>
      <el-button data-test="refresh" @click="load">刷新</el-button>
    </div>

    <el-table
      v-loading="loading"
      :data="items"
      data-test="todos-table"
      @row-click="openDetail"
      @selection-change="onSelectionChange"
    >
      <el-table-column v-if="box === 'todo' && auth.canApprove" type="selection" width="42" />
      <el-table-column prop="obj_no" label="图号" min-width="140" data-test="col-obj-no" />
      <el-table-column label="名称" min-width="140">
        <template #default="{ row }">{{ row.name ?? "-" }}</template>
      </el-table-column>
      <el-table-column v-if="box !== 'done'" label="版本" width="80">
        <template #default="{ row }">v{{ row.major_ver ?? "?" }}.{{ row.minor_ver ?? 0 }}</template>
      </el-table-column>
      <el-table-column v-if="box === 'todo'" label="提交人" width="100">
        <template #default="{ row }">{{ row.submitter ?? "-" }}</template>
      </el-table-column>
      <el-table-column label="密级" width="80">
        <template #default="{ row }">
          <el-tag
            :type="(row.secret_level ?? 0) >= 3 ? 'danger' : (row.secret_level ?? 0) === 2 ? 'warning' : 'info'"
            effect="plain"
            size="small"
          >
            {{ row.secret_level ?? 0 }}
          </el-tag>
        </template>
      </el-table-column>
      <el-table-column label="状态" width="100">
        <template #default="{ row }">
          <el-tag v-if="row.state" :type="stateTagType(row.state)">{{ stateLabel(row.state) }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column v-if="box === 'done'" label="动作" width="100">
        <template #default="{ row }">{{ row.action }}</template>
      </el-table-column>
      <el-table-column label="时间" min-width="160">
        <template #default="{ row }">{{ row.updated_at ?? row.at ?? "-" }}</template>
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
  </div>
</template>
