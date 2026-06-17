# UI Architecture

The client UI is built with `lurq`.

## Main Features Used

- Component system.
- Router.
- Modals and overlays.
- Resources and i18n.
- WGPU/DX12 rendering.
- Clipboard, SVG, markdown, forms, persistent storage, and devtools features.

## Screen Groups

| Path | Screens |
| --- | --- |
| `ui/identity_setup` | Identity creation. |
| `ui/loading_identity` | Startup and identity loading. |
| `ui/lobby` | Connected server, channel rail, chat, stream modal, stream watching. |
| `ui/settings` | Settings shell and all settings pages. |
| `ui/servers` | Saved server selection. |
| `ui/common` | Shared controls and modals. |

## App Chrome

Custom app chrome is enabled on Windows and macOS. The chrome layer owns titlebar behavior, window controls, border strips, resize handles, and native window drag/resize integration.

