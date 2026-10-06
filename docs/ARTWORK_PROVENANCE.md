# Vellora artwork provenance

Recorded October 4, 2026 for the private 0.2.20 publication candidate. Felix
Seitzer explicitly requested replacement of the earlier logo with a newly
generated notebook/quill/waveform concept and its use throughout the project.

## Current assets

The transparent master was generated through Codex's built-in OpenAI image
generation tool from the text prompt below. No earlier Vellora image, peer
logo, third-party image, uploaded photograph or personal data was supplied as
an image input. The paper variant was then created by the same built-in tool,
using **only that newly generated transparent master** as its edit target.

The tool did not expose an exact model identifier. This record identifies the
service/tool actually used rather than inventing a model name or claiming a
historical ChatGPT origin.

| Asset | Derivation | SHA-256 |
| --- | --- | --- |
| `src/assets/brand/vellora-logo-source.png` | Generated transparent 1254 × 1254 RGBA master | `6ce5be480e7b21f21b504559fef0cde98a932dd2b043d2a6566de7513dbabc30` |
| `src/assets/brand/vellora-symbol.png` | Byte-identical copy of the transparent master for the sidebar | `6ce5be480e7b21f21b504559fef0cde98a932dd2b043d2a6566de7513dbabc30` |
| `src/assets/brand/vellora-mark-paper.png` | Generated opaque ivory-background variant, 1254 × 1254 RGB | `63ea3bcf2cc851c3ea4f7f300d49cec7241dc2670ef66983b20d4274d6f238ca` |
| `src-tauri/icons/32x32.png` | Tauri icon CLI resize of the paper variant | `55abd7a5fe4d810efb1cf0b499d0314846bde35aad01c61359b5572819382337` |
| `src-tauri/icons/128x128.png` | Tauri icon CLI resize of the paper variant | `ae076450ee83af52b194dd7673e1f9ba216f7a93ed4c9d1c2d2fe7aa05904a1a` |
| `src-tauri/icons/128x128@2x.png` | Tauri icon CLI resize of the paper variant | `915b769a111b6debc738b3be96b1e94ba71a4c6eb461e8c3b6ddcee147f62175` |
| `src-tauri/icons/icon.png` | Tauri icon CLI 512 × 512 paper variant, used by the tray | `9e518efb84abd3da907e168ad33920d1897b78ed7270d9dd0772248dbe6f8830` |
| `src-tauri/icons/icon.ico` | Tauri icon CLI Windows multi-size icon from the paper variant | `84a3305ba310b499967e19ce8e9bb859520b547d33943e5ca542d9389e898697` |

PNG inspection found no EXIF, XMP, textual, author or directory metadata.
ICO metadata consists of its size inventory. Genuine alpha is present in the
transparent master; the paper variant is opaque so its dark ink remains visible
on Windows taskbar backgrounds. Only the five existing Windows icon files are
used and tracked; temporary generator outputs do not add platform support.

The original generation PNGs are retained locally, and their exact selected
bytes are committed above. No image was extracted from the old logo to create
the new one. The shared generic motifs and warm stationery aesthetic were
described in text at the owner's request.

## Exact master generation prompt

```text
Use case: logo-brand
Asset type: A new original square raster brand mark for Vellora, a Windows speech-to-text app, to use in its sidebar, dashboard and Windows app/tray icons.
Primary request: A warm, understated notebook-and-quill emblem with an audio waveform becoming the pen's ink stroke. Generate from this text description alone.
Subject and composition: Center one open notebook represented by a rounded rectangular page-outline, with a subtle shallow V at the bottom indicating the two pages. The upper edge has a tasteful gap. Inside the left half, a flowing speech waveform with three rounded peaks sweeps down into the nib of a diagonal feather quill rising toward the upper right. The nib and feather belong to one continuous, clearly readable quill. Add one small muted terracotta recording dot in the open upper-right area. Keep the notebook/quill/waveform coherent as one compact emblem, occupying about 80% of the square, with generous even margins.
Style: Elegant minimal hand-inked line art, clean simplified curves with a slight organic quality, flat and vector-friendly. Strong enough strokes to remain legible at 32px; do not use delicate hairlines. Warm dark brown ink, approximately #3C2B1F, and a small burnt-terracotta accent approximately #C56E48. Interior negative space is transparent.
Background: Genuine transparent alpha throughout the blank area. No paper rectangle, scenery, drop shadow or black/white backdrop.
Text: None. No letters, brand word, watermark, signature or extra symbols.
Constraints: One finished logo only, square composition. A newly drawn arrangement using the generic notebook, quill and waveform motifs. No reference image or third-party logo is supplied. Preserve the restrained warm stationery aesthetic and make the result usable as an application icon.
```

## Exact paper-variant edit prompt

```text
Use case: precise-object-edit
Asset type: Windows app icon and paper-background variant of the newly generated Vellora logo.
Input image: the provided PNG is the EDIT TARGET, a documented logo generated in this session.
Primary request: Keep this exact notebook, quill, speech-waveform and terracotta dot composition unchanged. Preserve the exact shapes, stroke widths, dark-brown and terracotta colors, positioning and margins. Change only the transparent background into an opaque, flat, warm ivory paper color #FAF7EF across the entire square, including the interior negative space. No texture, shadow, border, rounded app tile, new details or text. The square output must be fully opaque. This is a background-only variant for small Windows icons.
```

## Reproducing Windows icon derivatives

With the locked frontend dependencies installed:

```powershell
npm run tauri -- icon src/assets/brand/vellora-mark-paper.png --output .local/new-brand-paper-icons
foreach ($iconName in @('32x32.png', '128x128.png', '128x128@2x.png', 'icon.png', 'icon.ico')) {
    Copy-Item -LiteralPath (Join-Path .local/new-brand-paper-icons $iconName) -Destination (Join-Path src-tauri/icons $iconName) -Force
}
```

This command creates image/icon assets; it does not compile an application
executable. Only the selected Windows files should be copied into the project.

## Rights and publication history

The current assets are offered with the project under MIT to the extent the
maintainer holds licensable rights. Documenting generation resolves their
recorded provenance; it does not establish exclusive copyright, trademark
clearance or a universal non-infringement guarantee. See the
[licensing review](LICENSING_REVIEW.md).

Earlier commits contain the previous artwork and screenshots. Replacing files
in the current tree does not erase those historical blobs. The prepared
publication approach retains the existing repository as a private audit archive
and uses a reviewed independent source snapshot with clean history for a future
public repository. No history rewrite, repository rename, new remote or public
publication has been performed. Those actions remain subject to the owner's
final publication approval.

The [screenshot record](images/README.md) documents synthetic refreshed images
of the application using the new artwork.
