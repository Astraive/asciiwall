# Accessibility, motion, and localization

## Accessibility is a release gate

Every control needs an accessible name, role, current value, bounds, focus order, and keyboard operation. egui has accessibility integration, but that does not establish that this application works with screen readers without testing. [S49](../research/source-register.md#s49) Test the selected version and platform combinations with real assistive technology.

Provide a nonanimated list view of the library, keyboard-navigable settings, visible focus, scalable text, high-contrast UI, and numeric alternatives to sliders. Color must not be the only signal for success, incompatibility, or an unsaved change.

## Motion policy

Respect system reduced-motion where available and allow an explicit override. macOS exposes a reduced-motion preference. [S48](../research/source-register.md#s48) Replace spatial/glitch/flash transitions with fade or immediate changes. Disable auto-playing gallery thumbnails by default; a library of dozens of simultaneous animations is wasteful and distracting.

Offer one action to pause all wallpapers and a safe startup mode. A wallpaper package cannot turn reduced-motion off. Avoid medical claims about photosensitivity; use conservative design and test guidance instead.

## Text and Unicode

UI localization is separate from the engine's single-cell glyph support. The interface can support translated/RTL text while the wallpaper's v1 custom charset still restricts unsupported combining, bidi-control, and multi-cell glyph sequences. Do not equate 'Unicode font' with full shaping support.

Use translation keys, parameterized messages, localized numbers/dates, and plural forms. Keep machine-readable CLI/JSON identifiers stable in English-like kebab/dotted keys; human-facing messages can be translated. User-created names remain Unicode text with display-length and control-character validation.

## Display scaling

UI scale and wallpaper cell density are independent. Moving the editor between monitors should update control scaling without changing the active wallpaper grid. Test large text, narrow windows, long translated labels, 200 percent scale, and keyboard-only operation.

## Privacy and previews

Screen-reader announcements should not expose hidden local file paths or window titles. Preview tools describing pointer/window effects use generic sample rectangles. Diagnostics can explain missing permission without pressuring the user to grant it.

## Acceptance tests

Keyboard-only import/apply/pause/export, screen-reader control values, no focus trap in modal dialogs, reduced-motion toggled while a transition runs, high-contrast theme, long translations, and error-message announcements. These are planned tests, not completed accessibility certification.
