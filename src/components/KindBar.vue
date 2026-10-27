<script setup lang="ts">
// Gestapelter Balken nach Dateikategorie. Segmente durch 2px Flächenabstand
// getrennt (keine Rahmen), Werte stehen zusätzlich im Tooltip und in der Tabelle.
import { computed } from "vue";
import type { KindSize } from "../api";
import { byCategory, cssVar } from "../categories";
import { formatBytes } from "../format";

const props = withDefaults(defineProps<{ kinds: KindSize[]; height?: number }>(), { height: 8 });

const segments = computed(() => {
  const parts = byCategory(props.kinds).filter((c) => c.size > 0);
  const total = parts.reduce((s, c) => s + c.size, 0);
  return parts.map((c) => ({ ...c, share: total ? c.size / total : 0 }));
});
</script>

<template>
  <div class="flex w-full gap-[2px] overflow-hidden rounded" :style="{ height: `${height}px` }">
    <div
      v-for="s in segments"
      :key="s.id"
      class="h-full min-w-[2px]"
      :style="{ flexGrow: s.share, flexBasis: 0, background: cssVar(s.id) }"
      :title="`${s.label}: ${formatBytes(s.size)} (${Math.round(s.share * 100)} %, ${s.count} Dateien)`"
    />
  </div>
</template>
