<script setup lang="ts">
// 工作台首页（MVP：统计卡片占位，后续接入看板）
</script>

<template>
  <div class="page">
    <el-row :gutter="16">
      <el-col :span="8">
        <el-card>图纸数据：见左侧「图纸」菜单</el-card>
      </el-col>
      <el-col :span="8">
        <el-card>待办：登录后处理审核（submit/approve）</el-card>
      </el-col>
      <el-col :span="8">
        <el-card>事件：发布后自动 Webhook 外投</el-card>
      </el-col>
    </el-row>
  </div>
</template>
