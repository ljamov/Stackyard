# Look and language

Both are in **Settings › Appearance**. They change only how the app looks; nothing is downloaded and
nothing runs in the background, and the defaults (dark, amber, Stackyard icons, Default density,
100% zoom, side bar on the left, panel at the bottom, English) are how Stackyard has always looked.

## Theme and accent color

- **Theme**: Dark, Light, High contrast, or **Match my computer**, which follows the light or dark
  setting of Windows, macOS or Linux and switches along with it. When the computer asks for more
  contrast (a Windows contrast theme, or **Increase contrast** on macOS), it picks High contrast.
- **Accent color**: amber (the default), blue, green, violet, rose or teal. It colors buttons,
  switches, links and highlights.

High contrast is black with bright borders, a thick orange focus outline and underlined links, so
nothing depends on telling two shades apart.

The choice is saved in `settings.toml` (see [Settings files](#settings-files)) and is applied before
the window first draws, so the app never flashes the other theme.

**Ctrl+K, then Ctrl+T** opens the theme picker: move through the themes with the arrow keys to see
each one on the whole window, Enter keeps it, Escape goes back to the one you had.

## VS Code themes

Any VS Code color theme works in Stackyard. Stackyard reads its colors (`editor.background`,
`sideBar.background`, `statusBar.background`, `button.background`, `focusBorder` and about 50 more)
and uses them for its own parts, so your editor and Stackyard can match.

1. Get the theme: either its `.json` file, or the whole extension as a `.vsix` file. On the
   theme's page at [marketplace.visualstudio.com](https://marketplace.visualstudio.com) or
   [open-vsx.org](https://open-vsx.org), click **Download Extension** (on the Marketplace it is in
   the right-hand column under **Resources**).
2. In Stackyard, open **Settings › Appearance** and click **Import a VS Code theme…** (or run
   **Theme: Import a VS Code theme…** from the palette).
3. Pick the file. Every color theme in it is added under **Added themes**, and the first one is
   shown right away.

An added theme brings its own accent color, so the accent swatches step aside while it is shown.
Colors a theme leaves out are worked out from its background and text, so a theme with only a
handful of colors still looks whole. Remove a theme with the **×** beside its name.

Added themes are files in the `themes` folder in Stackyard's folder (e.g. `C:\stackyard\themes`),
which **Open the themes folder** shows. You can also copy a VS Code theme's `.json` there yourself,
or write your own:

```json
{
  "name": "My theme",
  "type": "dark",
  "colors": {
    "editor.background": "#1b1d23",
    "editor.foreground": "#d4d7dd",
    "button.background": "#e8a317",
    "statusBar.background": "#14161a"
  }
}
```

`type` is `dark`, `light` or `hc` (high contrast); comments and `include` (a theme building on
another file) work as in VS Code. Stackyard's own token names (see below) can be used as keys too,
and win over the VS Code ones. A file Stackyard can't read is listed in the **Problems** panel. Only
the colors of the window are used; the syntax colors of a theme are for code editors and are left
out.

## Icons, density, zoom and layout

- **Icons**: **Stackyard** (line icons drawn for the app) or **Codicons**, the icon font VS Code
  uses. Codicons loads only when you pick it.
- **Density**: **Compact** fits more rows and buttons on screen, **Comfortable** gives everything
  more room, **Default** is in between.
- **Zoom**: 80% to 150% for the whole window. **Ctrl+=** zooms in, **Ctrl+-** out and **Ctrl+0**
  goes back to 100%.
- **Side bar position**: the bar of icons and the Explorer on the **Left** or the **Right**.
- **Panel position**: the Logs and Problems panel at the **Bottom** or on the **Right**, beside the
  screen. On the right, drag the line beside it to change its width.

The palette has a command for each of these, for example **View: Move the side bar to the right**.

### For contributors: color tokens

Every color in the interface is a named token in `src/lib/theme.ts`, such as `sideBar.background`
or `status.error`. Each theme gives every token a value, and the app writes them as CSS variables
(`--sidebar`, `--error`) that `styles.css` and the components use. To color something new, use an
existing variable or add a token with a value in all three themes, and say in `VSCODE_KEYS` which VS
Code colors it should follow; don't write a color anywhere else. `npm run build` runs `scripts/check-colors.mjs`, which fails if a color value appears in `src`
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

## Commands and shortcuts

Everything the window can do is a command: open a screen, start or stop a service, open a site in
the browser, switch theme or accent color, close tabs, check for updates.

- **Ctrl+Shift+P** (or **F1**) opens the command palette. Type a few letters of a command, for
  example `start all` for "Start all services", then Enter. The commands you ran last come first.
- **Ctrl+P**, or the search box at the top of the window, finds a site or a screen. Enter opens the
  site in your browser or the screen as a tab. Type `>` first to search commands instead.

| Shortcut (⌘ instead of Ctrl on a Mac) | Does |
| --- | --- |
| Ctrl+Shift+P, F1 | Show all commands |
| Ctrl+P | Go to a site or screen |
| Ctrl+B | Show or hide the Explorer |
| Ctrl+J | Show or hide the Logs panel |
| Ctrl+W | Close the tab |
| Ctrl+Tab, Ctrl+PageDown | Next tab |
| Ctrl+Shift+Tab, Ctrl+PageUp | Previous tab |
| Ctrl+, | Open Settings |
| Ctrl+K, then Ctrl+S | Open Keyboard shortcuts |
| Ctrl+K, then Ctrl+T | Pick a color theme |
| Ctrl+=, Ctrl+- | Zoom in, zoom out |
| Ctrl+0 | Reset zoom |

To change a shortcut, open **Keyboard shortcuts** (from Settings, the palette or Ctrl+K Ctrl+S),
click **Change** beside a command and press the new keys; Escape cancels. **Remove** leaves the
command without keys and **Reset** brings back its default. If two commands share a key, both rows
say so. Your shortcuts are saved in `keybindings.toml`.

| Shortcut | Does |
| --- | --- |
| Ctrl+Shift+M | Show the Problems panel |

## Settings files

Two text files in Stackyard's folder (for example `C:\stackyard`, or `~/Library/Application
Support/Stackyard` on macOS) hold everything you can set. Change them in the app or in any text
editor; Stackyard picks up a change made in an editor within a few seconds, with no restart.
**Settings › Settings files** shows each one in your file manager.

**`settings.toml`** holds every setting on the Settings page. The look and language are in its
`[interface]` section:

```toml
[interface]
theme = "light"            # "dark", "light", "high-contrast", "system", or an added theme's file name
accent = "teal"            # "amber", "blue", "green", "violet", "rose" or "teal"
language = "mk"            # a language code, or "system"
iconTheme = "codicons"     # "stackyard" or "codicons"
density = "compact"        # "compact", "default" or "comfortable"
zoom = 110                 # 80 to 150, in steps of 10
sideBarPosition = "right"  # "left" or "right"
panelPosition = "right"    # "bottom" or "right"
```

A setting left out uses its default (dark, amber, English, Stackyard icons, Default density, 100%,
left, bottom). An added theme is named by its file in the `themes` folder without `.json`: `theme =
"one-dark-pro"` for `themes/one-dark-pro.json`.

**`keybindings.toml`** holds the shortcuts you changed, by command. Each line replaces that command's
default keys; `[]` leaves it without any. Command ids look like `view.explorer`, `open.Settings`,
`service.start.mysql` or `theme.light`; the ones with default keys are listed in `DEFAULT_KEYS` in
`src/lib/keybindings.ts`. The easiest start is to change a key on the Keyboard shortcuts screen and
look at what Stackyard wrote.

```toml
[keys]
"view.explorer" = ["ctrl+shift+e"]
"workbench.commands" = ["ctrl+shift+p", "f1"]
"tab.closeAll" = []
```

Write keys in lowercase, joined by `+`, with `ctrl`, `alt` and `shift` before the key (`ctrl` is ⌘
on a Mac). Two keys pressed one after the other are separated by a space: `"ctrl+k ctrl+s"`.

If something in either file can't be used (a typo in a value, a setting Stackyard doesn't know, a key
written wrongly, or a file that isn't valid TOML), Stackyard keeps working with the default or the
last good value and lists the problem in the **Problems** panel (Ctrl+Shift+M, or the count in the
status bar), with the file and what to fix. Comments you write in these files are not kept when you
change a setting in the app, because the app writes the file again.

### For contributors: adding a command

Commands are built in `useWorkbenchCommands` in `src/lib/commands.tsx`, each with an `id`, a
translated `title` and what it runs; the palette and the Keyboard shortcuts screen list them from
there. A default key goes in `DEFAULT_KEYS` in `src/lib/keybindings.ts`, by the command's `id`.

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
