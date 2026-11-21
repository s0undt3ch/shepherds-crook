# crook logo

**The flock.** A shepherd's crook watching over three sessions. The orange one is the session that's waiting on you.

The logo was made with AI image generation, then cleaned up by hand.

| File | Use |
|---|---|
| `crook-icon.svg` | Icon, 32 px and up. Switches ink colour with the viewer's light/dark scheme |
| `crook-icon-16.svg` / `favicon.svg` | 16 px version, redrawn on the pixel grid (2 px strokes, no tip flick) |
| `crook-lockup.svg` | Icon plus the "crook" wordmark (JetBrains Mono Bold, outlined) |
| `*-light.svg` / `*-dark.svg` | Fixed ink colour for light or dark backgrounds |
| `*-mono.svg` | Single colour (`currentColor`, accent included), for one-ink contexts |
| `crook-social-dark.png` | GitHub social preview, 1280×640 (light variant also included) |
| `favicon.ico` | 16 px (pixel-grid version), 32 px and 48 px |
| `concepts/B-flock.svg` | The original concept this logo grew from |

## Colours

| Token | Hex | Notes |
|---|---|---|
| crook orange (accent) | `#E8601F` | 3.4:1 contrast on white, 5.5:1 on `#0D1117` |
| ink, light backgrounds | `#1B1F24` | |
| ink, dark backgrounds | `#E6EDF3` | |

Use the accent only on the waiting sheep. Don't recolour the crook.

## README header

```html
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo/crook-lockup-dark.svg">
  <img alt="crook" src="assets/logo/crook-lockup-light.svg" width="234">
</picture>
```
