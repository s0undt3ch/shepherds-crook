# crook logo

**The flock.** A shepherd's crook watching over three sessions. The orange one is the session that's waiting on you.

The logo was made with AI image generation, then cleaned up by hand.

Fixed-ink files only read well on a matching background, and the mono files draw black in a
browser, so some previews below look faint in the other scheme.

| Preview | File | Use |
|---|---|---|
| <img src="crook-icon.svg" width="64" alt=""> | `crook-icon.svg` | Icon, 32 px and up. Switches ink colour with the viewer's light/dark scheme |
| <img src="crook-icon-light.svg" width="64" alt=""> | `crook-icon-light.svg` | Icon, fixed ink for light backgrounds |
| <img src="crook-icon-dark.svg" width="64" alt=""> | `crook-icon-dark.svg` | Icon, fixed ink for dark backgrounds |
| <img src="crook-icon-mono.svg" width="64" alt=""> | `crook-icon-mono.svg` | Icon, single colour (`currentColor`, accent included), for one-ink contexts |
| <img src="crook-icon-16.svg" width="32" alt=""> | `crook-icon-16.svg` | 16 px version, redrawn on the pixel grid (2 px strokes, no tip flick) |
| <img src="crook-icon-16-light.svg" width="32" alt=""> | `crook-icon-16-light.svg` | 16 px, light backgrounds |
| <img src="crook-icon-16-dark.svg" width="32" alt=""> | `crook-icon-16-dark.svg` | 16 px, dark backgrounds |
| <img src="crook-icon-16-mono.svg" width="32" alt=""> | `crook-icon-16-mono.svg` | 16 px, single colour |
| <img src="favicon.svg" width="32" alt=""> | `favicon.svg` | Same as `crook-icon-16.svg` |
| <img src="favicon.ico" width="32" alt=""> | `favicon.ico` | 16 px (pixel-grid version), 32 px and 48 px |
| <img src="crook-icon-512-light.png" width="64" alt=""> | `crook-icon-512-light.png` | 512 px PNG, light backgrounds |
| <img src="crook-icon-512-dark.png" width="64" alt=""> | `crook-icon-512-dark.png` | 512 px PNG, dark backgrounds |
| <img src="crook-lockup.svg" width="234" alt=""> | `crook-lockup.svg` | Icon plus the "crook" wordmark (JetBrains Mono Bold, outlined). Switches with the scheme |
| <img src="crook-lockup-light.svg" width="234" alt=""> | `crook-lockup-light.svg` | Lockup, light backgrounds |
| <img src="crook-lockup-dark.svg" width="234" alt=""> | `crook-lockup-dark.svg` | Lockup, dark backgrounds |
| <img src="crook-lockup-mono.svg" width="234" alt=""> | `crook-lockup-mono.svg` | Lockup, single colour |
| <img src="crook-social-dark.png" width="320" alt=""> | `crook-social-dark.png` | GitHub social preview, 1280×640 |
| <img src="crook-social-light.png" width="320" alt=""> | `crook-social-light.png` | Social preview, light variant |
| <img src="crook-social-dark.svg" width="320" alt=""> | `crook-social-dark.svg` | Source for the dark social preview |
| <img src="crook-social-light.svg" width="320" alt=""> | `crook-social-light.svg` | Source for the light social preview |
| <img src="concepts/B-flock.svg" width="64" alt=""> | `concepts/B-flock.svg` | The original concept this logo grew from |

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
