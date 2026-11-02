---
name: kiln-slint-tv
description: Enforces 10-foot TV UI ergonomics, Slint declarative guidelines, FocusScope gamepad routing, and off-thread image rendering.
---

# Kiln Slint 10-Foot TV UI Guidelines

The Kiln Shell (`kiln-shell`) is a fullscreen, controller-first 10-foot interface designed for viewing on living room televisions at distances of 6 to 10 feet.

## Core Design Principles

1. **Typography & Readability:**
   - Minimum body font size: 28px at 1080p (scaled proportionally at 4K).
   - High contrast ratios (minimum 4.5:1 for body text, >= 7:1 for headers).
   - 5% safe overscan margins on all display edges.

2. **Controller-First Navigation (`FocusScope`):**
   - Every interactive element must have clear, high-visibility focus states.
   - Recommended focus animation: slight scale up (`1.0 -> 1.06`) with a distinct accent focus ring (120ms ease-out).
   - Route keyboard arrows and XInput D-Pad / stick events through Slint's `FocusScope`.
   - Never require a mouse cursor to reach any functional screen or setting.

3. **Performance & Idle Resource Conservation:**
   - Slint renders on-demand; target ~0% CPU when idle on the home screen.
   - Zero continuous idle polling timers or background repaints on the UI thread.
   - Cursor auto-hides after 3 seconds of inactivity.

4. **Off-Thread Image Pipeline:**
   - Never decode or resize poster art on the Slint UI thread.
   - Worker threads in `kiln-library` decode and resize images using the `image` crate into RGBA8 buffers.
   - Hand off to UI via `slint::SharedPixelBuffer<slint::Rgba8Pixel>` and `slint::Image::from_rgba8_premultiplied`.
   - Virtualize large grid views so thousands of titles can be browsed with low memory footprint.
