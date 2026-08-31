<script lang="ts">
  /**
   * Linked images and their print resolution.
   *
   * Effective PPI is the number that decides whether a layout can go to press,
   * so it is the most prominent thing on each row — a picture enlarged past
   * its resolution looks fine on screen and soft on paper, and this panel is
   * where that gets caught.
   */
  import { open } from "@tauri-apps/plugin-dialog";
  import * as ipc from "$lib/ipc";
  import type { AssetSummary } from "$lib/ipc";
  import { studio } from "$lib/state.svelte";

  /** Below this, an image is too soft for commercial print. */
  const PPI_PRINT_FLOOR = 300;
  /** Below this it will look poor even in casual output. */
  const PPI_BAD = 150;

  let assets = $state<AssetSummary[]>([]);
  let busy = $state(false);

  $effect(() => {
    // Re-read whenever the document changes: placing, deleting or resizing an
    // image all move the resolution figure.
    void studio.tree;
    refresh();
  });

  async function refresh() {
    try {
      assets = await ipc.listLinkedAssets();
    } catch {
      assets = [];
    }
  }

  const IMAGE_FILTER = [
    { name: "Images", extensions: ["jpg", "jpeg", "png", "tif", "tiff", "webp"] },
  ];

  async function placeImage() {
    busy = true;
    try {
      const chosen = await open({ multiple: false, filters: IMAGE_FILTER });
      if (typeof chosen !== "string") return;
      const id = await ipc.placeImage(chosen, 60, 60);
      studio.select(id);
      await studio.invalidate(true);
    } catch (err) {
      console.error("could not place image:", err);
    } finally {
      busy = false;
    }
  }

  async function relink(asset: AssetSummary) {
    try {
      const chosen = await open({ multiple: false, filters: IMAGE_FILTER });
      if (typeof chosen !== "string") return;
      await ipc.relinkImage(asset.entity_id, chosen);
      await studio.invalidate(true);
    } catch (err) {
      console.error("could not relink:", err);
    }
  }

  function ppiClass(asset: AssetSummary): string {
    if (asset.status === "Missing") return "bad";
    if (asset.effective_ppi < PPI_BAD) return "bad";
    if (asset.effective_ppi < PPI_PRINT_FLOOR) return "warn";
    return "good";
  }
</script>

<section class="panel card">
  <header class="head">
    <h2>Assets</h2>
    <button class="chip" onclick={placeImage} disabled={busy}>Place image…</button>
  </header>

  {#if assets.length === 0}
    <p class="empty">No images linked yet.</p>
  {:else}
    <ul class="list">
      {#each assets as asset (asset.entity_id)}
        <li>
          <div
            class="asset"
            class:selected={studio.selectedEntityId === asset.entity_id}
            role="option"
            tabindex="0"
            aria-selected={studio.selectedEntityId === asset.entity_id}
            onclick={() => studio.select(asset.entity_id)}
            onkeydown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                studio.select(asset.entity_id);
              }
            }}
          >
            <div class="row">
              <span class="file" title={asset.path}>{asset.file_name}</span>
              <span class="ppi {ppiClass(asset)}">
                {asset.status === "Missing" ? "missing" : `${Math.round(asset.effective_ppi)} PPI`}
              </span>
            </div>
            <div class="row sub">
              <span>{asset.natural_width} × {asset.natural_height} px</span>
              <button
                class="link"
                onclick={(e) => {
                  e.stopPropagation();
                  relink(asset);
                }}
              >
                Relink…
              </button>
            </div>
            {#if asset.status === "Ok" && asset.effective_ppi < PPI_PRINT_FLOOR}
              <p class="note">
                Below {PPI_PRINT_FLOOR} PPI — enlarged past its resolution for print.
              </p>
            {/if}
          </div>
        </li>
      {/each}
    </ul>
  {/if}
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
    gap: 0.7rem;
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

  .chip {
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 5px;
    color: #cbd5e1;
    font-size: 0.68rem;
    font-weight: 600;
    padding: 0.28rem 0.55rem;
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

  .empty {
    margin: 0;
    font-size: 0.75rem;
    color: #475569;
  }

  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    max-height: 14rem;
    overflow-y: auto;
  }

  .asset {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    padding: 0.45rem 0.55rem;
    border-radius: 6px;
    border: 1px solid transparent;
    background: rgba(255, 255, 255, 0.03);
    cursor: pointer;
    transition: background 0.12s;
  }

  .asset:hover {
    background: rgba(255, 255, 255, 0.06);
  }

  .asset:focus-visible {
    outline: 2px solid #38bdf8;
    outline-offset: -1px;
  }

  .asset.selected {
    background: rgba(56, 189, 248, 0.12);
    border-color: rgba(56, 189, 248, 0.35);
  }

  .row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
  }

  .file {
    font-size: 0.78rem;
    color: #e2e8f0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ppi {
    font-size: 0.68rem;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    padding: 0.08rem 0.35rem;
    border-radius: 3px;
  }

  .ppi.good {
    color: #4ade80;
    background: rgba(74, 222, 128, 0.12);
  }

  .ppi.warn {
    color: #fbbf24;
    background: rgba(251, 191, 36, 0.12);
  }

  .ppi.bad {
    color: #f87171;
    background: rgba(248, 113, 113, 0.14);
  }

  .sub {
    font-size: 0.68rem;
    color: #64748b;
    font-variant-numeric: tabular-nums;
  }

  .link {
    background: none;
    border: none;
    padding: 0;
    color: #64748b;
    font-size: 0.68rem;
    font-weight: 600;
    cursor: pointer;
  }

  .link:hover {
    color: #38bdf8;
    text-decoration: underline;
  }

  .note {
    margin: 0.15rem 0 0;
    font-size: 0.67rem;
    color: #fbbf24;
    line-height: 1.35;
  }
</style>
