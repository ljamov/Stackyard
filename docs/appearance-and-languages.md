# Look and language

Both are in **Settings › Appearance**. They change only how the app looks; nothing is downloaded and
nothing runs in the background, and the defaults (dark, amber, English) are how Stackyard has always
looked.

## Theme and accent color

- **Theme**: Dark, Light, High contrast, or **Match my computer**, which follows the light or dark
  setting of Windows, macOS or Linux and switches along with it. When the computer asks for more
  contrast (a Windows contrast theme, or **Increase contrast** on macOS), it picks High contrast.
- **Accent color**: amber (the default), blue, green, violet, rose or teal. It colors buttons,
  switches, links and highlights.

High contrast is black with bright borders, a thick orange focus outline and underlined links, so
nothing depends on telling two shades apart.

The choice is kept with the app's window settings on this computer and is applied before the
window first draws, so the app never flashes the other theme.

### For contributors: color tokens

Every color in the interface is a named token in `src/lib/theme.ts`, such as `sideBar.background`
or `status.error`. Each theme gives every token a value, and the app writes them as CSS variables
(`--sidebar`, `--error`) that `styles.css` and the components use. To color something new, use an
existing variable or add a token with a value in all three themes; don't write a color anywhere
else. `npm run build` runs `scripts/check-colors.mjs`, which fails if a color value appears in `src`
outside `theme.ts`.

## The window

| Part | What it does |
| --- | --- |
| Bar of icons on the left | The top icon shows or hides the Explorer. Each other icon opens a screen as a tab; the Logs icon near the bottom shows or hides the Logs panel. |
| Explorer | Your services, each with a start or stop button, and your sites. Click a name to open its screen. |
| Tabs | Every screen you open stays open as a tab until you close it. Middle-click a tab to close it. |
| Logs panel | Every service's log along the bottom, under whichever screen is open. |
| Status bar | How many services are running and how many sites you have; click either to open its screen. When an update is out, it says so there too. |

Drag the line beside the Explorer or above the Logs panel to resize it, or focus the line with Tab and
use the arrow keys. Which parts show, their sizes and the open tabs are kept, so Stackyard opens the
way you left it. In a narrow window the Explorer steps aside to leave room for the screen.

| Shortcut (⌘ instead of Ctrl on a Mac) | Does |
| --- | --- |
| Ctrl+B | Show or hide the Explorer |
| Ctrl+J | Show or hide the Logs panel |
| Ctrl+W | Close the tab |
| Ctrl+Tab, Ctrl+PageDown | Next tab |
| Ctrl+Shift+Tab, Ctrl+PageUp | Previous tab |

## Language

Pick a language from the list, or **Match my computer** to use your computer's language when
Stackyard has it (English otherwise). Stackyard comes with:

| Language | File |
| --- | --- |
| English | built in |
| Deutsch | `src/locales/de.json` |
| Español | `src/locales/es.json` |
| Français | `src/locales/fr.json` |
| Italiano | `src/locales/it.json` |
| Македонски (Macedonian) | `src/locales/mk.json` |
| Português (Brasil) | `src/locales/pt-BR.json` |

The menus, the main screens' titles, the Dashboard, the Services screen, Settings and update
notices are translated. The rest of the app, the tray menu and the `stackyard` command stay in
English for now; anything not translated shows in English rather than as a blank.

### Add a language or change a word

You don't need to build Stackyard for this.

1. Open **Settings**, and under **Appearance** click **Add a language…**. A folder named
   `languages` opens (inside Stackyard's folder, e.g. `C:\stackyard\languages`).
2. Make a text file in it named after the language's code with `.json` at the end, for example
   `nl.json` for Dutch or `pt-PT.json` for Portuguese (Portugal). On Windows, turn on
   **View › File name extensions** in File Explorer first so the file doesn't end up as
   `nl.json.txt`.
3. Open it in Notepad and write the English text on the left and your translation on the right.
   `_name` is the language's own name, shown in the list:

   ```json
   {
     "_name": "Nederlands",
     "Settings": "Instellingen",
     "Sites": "Sites",
     "New site": "Nieuwe site",
     "Install {version} and restart": "{version} installeren en opnieuw starten"
   }
   ```

   Words in `{curly brackets}` are filled in by the app (a version, a name, a number); keep them
   as they are. The easiest start is to copy one of the files listed above and replace the
   right-hand side of each line.
4. Save the file as UTF-8 (Notepad does this by default), go back to Stackyard and click
   **Reload language files**. Your language appears in the list.

A file with the same code as a built-in language (say `de.json`) doesn't replace it; it only
changes the words it contains, so you can fix one word without copying the whole file. Files that
aren't valid JSON, aren't named after a language code, or are larger than 1 MB are skipped.

### For contributors

Text in the interface is written in English inside `t("…")`; see `src/lib/i18n.tsx`. To see what a
language file is missing, or to get every text as an empty template for a new language:

```sh
node scripts/i18n.mjs        # how complete each file in src/locales is
node scripts/i18n.mjs keys   # every text, as JSON with empty translations
```

A new built-in language is one file in `src/locales` plus a line in `BUILT_IN` in
`src/lib/i18n.tsx`. Each language loads only when it is picked, so more languages don't make the
app slower to start.
