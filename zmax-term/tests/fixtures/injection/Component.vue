<template>
  <div class="wrap">{{ label }}</div>
</template>

<script lang="ts">
export default {
  data() {
    return { label: "vue label" as string };
  },
};
</script>

<style lang="scss">
.wrap {
  color: vuegreen;
  .inner { padding: 2px; }
}
</style>
