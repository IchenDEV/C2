---
id: 2026-09-28-composer-reference-layout
schema: 5
stage: intent
status: accepted
owner: Codex
created: 2026-09-28
source: user
risk: medium
approved_by: chenli
approved_at: 2026-09-28
approval_source: "User in this chat: 参考这个改进输入框, with seven visual references."
---

# Intent: Composer Reference Layout

## Intent

Improve the desktop prompt composer using the seven supplied reference images: generous writing space, direct model/reasoning/permission controls, right-aligned attachment and submit actions, and a quieter checkout footer. Preserve editor drafts, provider-owned choices, permissions and session/worktree lifecycle. Local implementation and checks are authorized. Do not invent server updates, remote hosts or service tiers from screenshot content. No release requested.

Follow-up authorization: chenli reported “对比度不行” in the same chat on 2026-09-28. Correct surface separation and control legibility within the existing composer scope.

Latest user direction (2026-09-28): “保持主输入框白色底，考虑从阴影、边框入手”. This supersedes the gray input fill and authorizes a composer-specific boundary/shadow design exception.

Final edge refinement (2026-09-28): user requests “改成0.5”; reduce the composer edge from 1px to 0.5px, retaining its fill and shadows.

Toolbar follow-up: user supplied a screenshot of the session header and explicitly chose “统一为柔和的中灰” on 2026-09-28. Apply the existing neutral toolbar design contract to text and icons, preserving disabled states.

Sidebar follow-up: chenli requested “左侧也统一柔和的中灰” in this chat on 2026-09-28. Extend the same tone to left navigation text and icons, preserving meaningful state indicators.

Composer-toolbar follow-up: user supplied the composer controls/checkout screenshot and requested “还有这些” on 2026-09-28. Extend the same soft medium gray to these labels and neutral action icons. This supersedes the earlier full-foreground control contrast choice.

PR authorization: chenli requested “pr” in this chat on 2026-09-28, authorizing a review branch, commit, push and pull request for this completed scope. Merge and release remain unrequested.
