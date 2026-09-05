# Unseat UX pass

Date: 2026-09-02
Status: Approved

## Goal

Make the floating timer usable: Pause/Reset stay clickable, Settings is reachable and actually saves, shape is mouse-selectable, face is easier to see.

## Changes

### Hover

Window size does **not** change on hover. Capsule/card/disc always reserve control space. Pause/Reset fade in on enter and out on leave. Confirm the cursor left the window before collapsing hover (ignore spurious `WM_MOUSELEAVE`).

Skip `SetWindowPos` when size is unchanged.

### Reset

Reset zeros sitting/break/overdue/beeps **and sets running = true** (timer starts again from `0:00`).

### Right-click

Right-click the timer opens Settings. Tray Settings remains. Quit stays tray-only.

### Settings

- Top-level, `WS_EX_TOPMOST`, **not** owned by the no-activate widget (so mouse activation works).
- Shape: three radio buttons (Capsule / Card / Disc), not a combo.
- Persist to `settings.json` on any control change. OK/X close the dialog.
- Do not disable the timer while Settings is open.

### Look

Same three shapes. Fill opacity ~0.94–0.96, ~20% larger digits, **2px** state-colored rim. No pulse, no neon. Hover buttons use an opaque accent chip.
