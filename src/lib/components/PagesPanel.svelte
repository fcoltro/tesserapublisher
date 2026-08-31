<script lang="ts">
  /**
   * Spreads and master pages.
   *
   * Thumbnails are proportioned from the real page geometry rather than drawn
   * at a fixed ratio, so a landscape document reads as landscape here. Pages
   * that sit together on a spread are grouped, with the spine between them.
   *
   * Dragging a master onto a page applies it, the way layout tools do it.
   */
  import * as ipc from "$lib/ipc";
  import type { MasterPageSummary, PagePlacement } from "$lib/ipc";
  import { studio } from "$lib/state.svelte";

  let masters = $state<MasterPageSummary[]>([]);
  let draggingMaster = $state<number | null>(null);
  let dropPage = $state<number | null>(null);
  let busy = $state(false);

  /** Pages grouped by the spread they belong to, in reading order. */
  let spreads = $derived.by(() => {
    const groups = new Map<number, PagePlacement[]>();
    for (const page of studio.pages) {
      const bucket = groups.get(page.spread_index) ?? [];
      bucket.push(page);
      groups.set(page.spread_index, bucket);
    }
    return [...groups.entries()]
      .sort((a, b) => a[0] - b[0])
      .map(([index, pages]) => ({
        index,
        pages: pages.sort((a, b) => a.page_number - b.page_number),
      }));
  });

  async function refreshMasters() {
    try {
      masters = await ipc.listMasterPages();
    } catch {
      masters = [];
    }
  }

  $effect(() => {
    // Re-read whenever the document changes; masters can be created elsewhere.
    void studio.tree;
    refreshMasters();
  });

  async function addPage() {
    busy = true;
    try {
      await ipc.addPage();
      await studio.invalidate();
    } finally {
      busy = false;
    }
  }

  async function removePage(pageNumber: number) {
    if (studio.pages.length <= 1) return;
    busy = true;
    try {
      await ipc.removePage(pageNumber);
      await studio.invalidate();
    } catch (err) {
      console.warn("could not remove page:", err);
    } finally {
      busy = false;
    }
  }

  async function applyMaster(pageNumber: number, masterId: number) {
    try {
      await ipc.applyMasterToPage(pageNumber, masterId);
      await studio.invalidate();
    } catch (err) {
      console.warn("could not apply master:", err);
    }
  }

  async function detachMaster(pageNumber: number) {
    try {
      await ipc.detachMasterFromPage(pageNumber);
      await studio.invalidate();
    } catch (err) {
      console.warn("could not detach master:", err);
    }
  }

  /** Thumbnail height for a page, from its real proportions. */
  function thumbHeight(page: PagePlacement): number {
    const WIDTH = 44;
    return Math.round((page.height / Math.max(page.width, 1)) * WIDTH);
  }
</script>

<section class="panel card">
  <header class="head">
    <h2>Pages</h2>
    <div class="actions">
      <button class="chip" onclick={addPage} disabled={busy}>+ Page</button>
      <button
        class="chip"
        onclick={() => removePage(studio.pages.length)}
        disabled={busy || studio.pages.length <= 1}
      >
        − Page
      </button>
    </div>
  </header>

  <div class="spreads">
    {#each spreads as spread (spread.index)}
      <div class="spread">
        {#each spread.pages as page (page.page_number)}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="page"
            class:drop={dropPage === page.page_number}
            ondragover={(e) => {
              if (draggingMaster === null) return;
              e.preventDefault();
              dropPage = page.page_number;
            }}
            ondragleave={() => (dropPage = null)}
            ondrop={() => {
              if (draggingMaster !== null) applyMaster(page.page_number, draggingMaster);
              draggingMaster = null;
              dropPage = null;
            }}
          >
            <div class="sheet" style="height: {thumbHeight(page)}px"></div>
            <span class="num">{page.page_number}</span>
          </div>
        {/each}
      </div>
    {/each}
  </div>

  <div class="masters">
    <h3>Masters</h3>
    {#if masters.length === 0}
      <p class="empty">No master pages yet.</p>
    {:else}
      <ul class="master-list">
        {#each masters as master (master.id)}
          <li>
            <div
              class="master"
              draggable="true"
              role="listitem"
              title="Drag onto a page to apply {master.name}"
              ondragstart={() => (draggingMaster = master.id)}
              ondragend={() => {
                draggingMaster = null;
                dropPage = null;
              }}
            >
              <span class="prefix">{master.prefix}</span>
              <span class="master-name">{master.name}</span>
            </div>
          </li>
        {/each}
      </ul>
      <p class="hint">Drag a master onto a page to apply it.</p>
      <button class="chip wide" onclick={() => detachMaster(studio.pages.length)}>
        Detach master from last page
      </button>
    {/if}
  </div>
</section>

<style>
  .card {
    background: rgba(15, 23, 42, 0.75);
    backdrop-filter: blur(14px);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 14px;
    padding: 1.15rem;
    box-shadow: 0 8px 30px rgba(0, 0, 0, 0.4);
    display: flex;
    flex-direction: column;
  }

  .panel {
    gap: 0.85rem;
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }

  h2 {
    margin: 0;
    font-size: 0.95rem;
    font-weight: 600;
  }

  .actions {
    display: flex;
    gap: 0.3rem;
  }

  .chip {
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 5px;
    color: #cbd5e1;
    font-size: 0.68rem;
    font-weight: 600;
    padding: 0.25rem 0.5rem;
    cursor: pointer;
    transition: all 0.15s;
  }

  .chip:hover:not(:disabled) {
    border-color: #38bdf8;
    color: #38bdf8;
  }

  .chip:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }

  .chip.wide {
    width: 100%;
    margin-top: 0.35rem;
  }

  .spreads {
    display: flex;
    flex-wrap: wrap;
    gap: 0.85rem;
    align-items: flex-start;
  }

  /* Pages of one spread meet at the spine, so no gap between them. */
  .spread {
    display: flex;
    gap: 1px;
    padding: 0.3rem;
    border-radius: 6px;
    background: rgba(0, 0, 0, 0.2);
  }

  .page {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.2rem;
    padding: 2px;
    border: 1px solid transparent;
    border-radius: 3px;
  }

  .page.drop {
    border-color: #38bdf8;
    border-style: dashed;
  }

  .sheet {
    width: 44px;
    background: #e8edf5;
    border-radius: 1px;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.35);
  }

  .num {
    font-size: 0.62rem;
    color: #64748b;
    font-variant-numeric: tabular-nums;
  }

  .masters {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding-top: 0.7rem;
    border-top: 1px solid rgba(255, 255, 255, 0.08);
  }

  h3 {
    margin: 0;
    font-size: 0.7rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: #64748b;
  }

  .empty,
  .hint {
    margin: 0;
    font-size: 0.7rem;
    color: #475569;
  }

  .master-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .master {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    padding: 0.3rem 0.4rem;
    border-radius: 5px;
    background: rgba(255, 255, 255, 0.03);
    cursor: grab;
    transition: background 0.15s;
  }

  .master:hover {
    background: rgba(56, 189, 248, 0.12);
  }

  .master:active {
    cursor: grabbing;
  }

  .prefix {
    font-size: 0.62rem;
    font-weight: 700;
    color: #38bdf8;
    background: rgba(56, 189, 248, 0.14);
    padding: 0.08rem 0.3rem;
    border-radius: 3px;
  }

  .master-name {
    font-size: 0.75rem;
    color: #cbd5e1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
