---
id: 2026-09-28-codex-layout-reset
schema: 5
stage: intent
status: accepted
owner: codex
created: 2026-09-28
source: user
risk: medium
approved_by: chenli
approved_at: 2026-09-28
approval_source: "User request in this chat: 参考Codex 重置 layout UX 框架, with reference screenshot."
---

# Intent: Codex Layout Reset

## Intent

Reorganize the desktop shell around the supplied Codex reference: persistent narrow app navigation, task sidebar, quiet workspace and bottom composer. Preserve task data, drafts, navigation features, provider behavior and existing pane/dock state. Bounded local UI work is authorized; no PR, native process replacement or release requested.

Follow-up direction (same requester, 2026-09-28): “参考这个需要一点不太张扬，但又很可靠的感觉，让 UI 框架按照我们之前就 OK 的。” The supplied T3 Code image guides restrained visual hierarchy; retain the previously agreed layout and behavior.

Further direction (same requester, 2026-09-28): “采用类似的浮动面板”. Apply restrained floating treatment to the workspace composer and right tool panel, retaining the established layout.

Corner refinement (same requester, 2026-09-28): “面板添加圆角”. Round the main workspace to match the already rounded tool panel, keeping the composer radius.

Shadow refinement (same requester, 2026-09-28): “统一面板的阴影”. Use one existing raised shadow for the main workspace, right tool panel and composer.

Top chrome refinement (same requester, 2026-09-28): “参考这个优化顶部表现”, with a compact Codex top-strip reference. Improve header density, title hierarchy and tool tab context while preserving native controls, drag routing and existing actions.

Sidebar chrome follow-up (same requester, 2026-09-28): “也移动上去，另外不要codetwo展示了”, with the sidebar brand/search/toggle crop. Move search and toggle to the window strip and remove visible CodeTwo branding there and in the rail.

Shadow weight follow-up (same requester, 2026-09-28): “现在阴影过重了，轻一点”. Reduce the shared raised shadow while keeping all floating workspace surfaces consistent.

Icon shape follow-up (same requester, 2026-09-28): “单Icon 按钮改成全圆”. Make standalone icon-only buttons circular throughout the shared controls and workspace, including responsive toolbar icons.

Visual simplification (same requester, 2026-09-28): “页面有点喧嚣，不够干净”. Reduce redundant new-task chrome and flatten plugin action surfaces, preserving named actions and descriptions on demand.

Project-entry refinement (same requester, 2026-09-28): “add a Project 的UX 很扎眼”. Make the empty-sidebar project entry blend into the task list.

Dock titlebar refinement (same requester, 2026-09-28): “右边面板的 tab和关闭按钮 提升到标题栏”, with a Codex reference. Move right-panel controls into the shared top strip, freeing panel content height.

Spacing refinement (same requester, 2026-09-28): “有一些间距问题需要修复”. Correct measured inconsistencies in top controls, inter-panel gutters and composer context spacing.

File-panel refinement (same requester, 2026-09-28): “这个地方显得很乱七八糟，而且上下的间距都不一样。有重复的那个导航栏。” The supplied crop identifies the empty file-tab row and mismatched search/action spacing beneath the Files surface tab.
