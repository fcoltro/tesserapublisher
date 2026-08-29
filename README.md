# Tessera Publish

High-performance visual publishing studio built with:
- **Tauri v2** for lightweight, secure native desktop integration.
- **Rust Backend** with **Bevy ECS (`bevy_ecs`)** for data-oriented document and entity state management.
- **Svelte 5 (Runes)**, **TypeScript**, and **Vite** for the reactive frontend UI.

## Architecture

- **`src-tauri/src/state.rs`**: Defines the Bevy ECS components (`Position { x, y }`, `Size { width, height }`) and `AppState` wrapping the thread-safe `std::sync::Mutex<World>`.
- **`src-tauri/src/lib.rs`**: Registers managed `AppState` in Tauri v2 and provides IPC commands for querying and spawning ECS entities.
- **`src/routes/+page.svelte`**: Svelte 5 application leveraging runes (`$state`, `$derived`, `$effect`) for interactive world monitoring and entity manipulation.

## Getting Started

### Install Dependencies
```bash
npm install
```

### Run in Development
```bash
npm run tauri dev
```

### Run Frontend Only (Vite)
```bash
npm run dev
```

### Build for Production
```bash
npm run tauri build
```
