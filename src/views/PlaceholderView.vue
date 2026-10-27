<script setup lang="ts">
import { useRoute } from "vue-router";
const route = useRoute();
</script>

<template>
  <section class="mx-auto max-w-3xl px-8 py-10">
    <h1 class="text-xl font-semibold tracking-tight">{{ route.meta.title }}</h1>
    <p class="mt-2 text-stone-500 dark:text-stone-400">Dieser Bereich entsteht in Phase {{ route.meta.phase }}.</p>
  </section>
</template>
