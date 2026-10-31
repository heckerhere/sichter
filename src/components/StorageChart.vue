<script setup lang="ts">
// Treemap bzw. Sunburst der Ordnergrößen. Klick zoomt in einen Unterordner,
// die Brotkrumen-Leiste führt zurück. Farbe = überwiegende Dateikategorie.
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import * as echarts from "echarts/core";
import { SunburstChart, TreemapChart } from "echarts/charts";
import { TooltipComponent } from "echarts/components";
import { CanvasRenderer } from "echarts/renderers";
import type { FolderRow, KindSize } from "../api";
import { byCategory, resolvedColor, type Category } from "../categories";
import { formatBytes, formatInt } from "../format";
import { locale, t } from "../i18n";

echarts.use([TreemapChart, SunburstChart, TooltipComponent, CanvasRenderer]);

const props = defineProps<{ folders: FolderRow[]; rootLabel: string; mode: "treemap" | "sunburst" }>();

const el = ref<HTMLDivElement | null>(null);
let chart: echarts.ECharts | null = null;

interface Node {
  name: string;
  path: string;
  value: number;
  files: number;
  kinds: KindSize[];
  children?: Node[];
  itemStyle: { color: string };
}

function dominant(kinds: KindSize[]): Category {
  const cats = byCategory(kinds);
  return cats.reduce((best, c) => (c.size > best.size ? c : best), cats[0]).id;
}

/** Größen der Dateien direkt im Ordner, nach Art (rekursiv minus Unterordner). */
function ownKinds(f: FolderRow, kids: FolderRow[]): KindSize[] {
  const map = new Map(f.kinds.map((k) => [k.kind, { ...k }]));
  for (const c of kids) {
    for (const k of c.kinds) {
      const e = map.get(k.kind);
      if (e) {
        e.size -= k.size;
        e.count -= k.count;
      }
    }
  }
  return [...map.values()].filter((k) => k.size > 0);
}

function buildTree(): Node | null {
  const children = new Map<number | null, FolderRow[]>();
  for (const f of props.folders) {
    const list = children.get(f.parent_id) ?? [];
    list.push(f);
    children.set(f.parent_id, list);
  }
  const toNode = (f: FolderRow): Node => {
    const kids = children.get(f.id) ?? [];
    const name = f.rel_path ? f.rel_path.slice(f.rel_path.lastIndexOf("/") + 1) : props.rootLabel;
    const node: Node = {
      name,
      path: f.rel_path || props.rootLabel,
      value: f.total_size,
      files: f.file_count,
      kinds: f.kinds,
      itemStyle: { color: resolvedColor(dominant(f.kinds)) },
    };
    if (kids.length) {
      node.children = kids.map(toNode);
      // Dateien direkt in diesem Ordner als eigene Kachel, sonst fehlen sie in der Fläche.
      if (f.own_size > 0) {
        const own = ownKinds(f, kids);
        node.children.push({
          name: t.storage.chart.filesHere,
          path: t.storage.chart.direct(f.rel_path || props.rootLabel),
          value: f.own_size,
          files: own.reduce((s, k) => s + k.count, 0),
          kinds: own,
          itemStyle: { color: resolvedColor(dominant(own)) },
        });
      }
    }
    return node;
  };
  const root = children.get(null)?.[0];
  return root ? toNode(root) : null;
}

function tooltip(info: { data: Node }): string {
  const n = info.data;
  if (!n?.kinds) return "";
  const rows = byCategory(n.kinds)
    .filter((c) => c.size > 0)
    .map(
      (c) =>
        `<div style="display:flex;gap:8px;align-items:center"><span style="width:8px;height:8px;border-radius:2px;background:${resolvedColor(c.id)}"></span>` +
        `<span style="flex:1">${c.label}</span><span style="font-variant-numeric:tabular-nums">${formatBytes(c.size)}</span></div>`,
    )
    .join("");
  return (
    `<div style="font-weight:600;margin-bottom:2px">${escapeHtml(n.path)}</div>` +
    `<div style="margin-bottom:6px;opacity:.75">${formatBytes(n.value)} · ${t.storage.files(formatInt(n.files))}</div>${rows}`
  );
}

function escapeHtml(s: string) {
  return s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);
}

function render() {
  if (!chart) return;
  const data = buildTree();
  if (!data) {
    chart.clear();
    return;
  }
  const dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  const surface = resolvedColor("surface");
  const text = dark ? "#f5f5f4" : "#1c1917";
  const label = {
    color: "#ffffff",
    textBorderColor: "rgba(0,0,0,0.45)",
    textBorderWidth: 2,
    fontSize: 11,
    overflow: "truncate" as const,
  };
  const tooltipStyle = {
    trigger: "item" as const,
    formatter: tooltip as never,
    backgroundColor: dark ? "#292524" : "#ffffff",
    borderColor: dark ? "#44403c" : "#e7e5e4",
    textStyle: { color: text, fontSize: 12 },
    extraCssText: "border-radius:8px;box-shadow:0 4px 16px rgba(0,0,0,.12);min-width:200px",
  };

  chart.setOption(
    props.mode === "treemap"
      ? {
          tooltip: tooltipStyle,
          series: [
            {
              type: "treemap",
              data: data.children ?? [data],
              name: data.name,
              leafDepth: 1,
              roam: false,
              nodeClick: "zoomToNode",
              squareRatio: 1.2,
              top: 0,
              left: 0,
              right: 0,
              bottom: 34,
              label: { ...label, formatter: (p: { name: string; value: number }) => `${p.name}\n${formatBytes(p.value)}` },
              upperLabel: { show: true, height: 22, ...label },
              itemStyle: { borderColor: surface, borderWidth: 2, gapWidth: 2, borderRadius: 4 },
              levels: [{ itemStyle: { borderWidth: 0, gapWidth: 2 } }, { itemStyle: { gapWidth: 2 } }],
              breadcrumb: {
                show: true,
                bottom: 4,
                left: 0,
                height: 22,
                itemStyle: { color: dark ? "#292524" : "#f5f5f4", borderColor: "transparent", textStyle: { color: text } },
                emphasis: { itemStyle: { color: dark ? "#44403c" : "#e7e5e4" } },
              },
            },
          ],
        }
      : {
          tooltip: tooltipStyle,
          series: [
            {
              type: "sunburst",
              data: data.children ?? [data],
              radius: ["12%", "95%"],
              nodeClick: "rootToNode",
              sort: undefined,
              itemStyle: { borderColor: surface, borderWidth: 2, borderRadius: 3 },
              label: { ...label, rotate: "radial", minAngle: 8 },
              emphasis: { focus: "ancestor" },
            },
          ],
        },
    true,
  );
}

let observer: ResizeObserver | null = null;
const scheme = window.matchMedia("(prefers-color-scheme: dark)");
const onScheme = () => render();

onMounted(() => {
  chart = echarts.init(el.value!);
  render();
  observer = new ResizeObserver(() => chart?.resize());
  observer.observe(el.value!);
  scheme.addEventListener("change", onScheme);
});
onBeforeUnmount(() => {
  observer?.disconnect();
  scheme.removeEventListener("change", onScheme);
  chart?.dispose();
});
watch(() => [props.folders, props.mode, locale.value], render);
</script>

<template>
  <div ref="el" class="h-full w-full" />
</template>
