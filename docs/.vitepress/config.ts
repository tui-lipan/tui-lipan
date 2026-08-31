import { defineConfig } from "vitepress";
import { fileURLToPath } from "node:url";
import { readFileSync } from "node:fs";
import { repoLinksPlugin } from "./repoLinks";

const srcDir = fileURLToPath(new URL("..", import.meta.url)).replace(/\/$/, "");

/** What the index page calls itself, in a tab and when shared. */
const LANDING_TITLE = "tui-lipan documentation";

/**
 * Read from the manifest rather than written down here. The version used to be
 * typed into the navbar chip, which is a chance for the site to advertise a
 * release that does not exist.
 */
const LIPAN_VERSION = (() => {
  const manifest = readFileSync(
    fileURLToPath(new URL("../../Cargo.toml", import.meta.url)),
    "utf8",
  );
  const found = /^version\s*=\s*"([^"]+)"/m.exec(manifest);
  if (!found) throw new Error("no version in Cargo.toml");
  return found[1];
})();

export default defineConfig({
  title: "tui-lipan",
  description: "Component-based TUI framework for Rust - full documentation.",
  cleanUrls: true,
  lastUpdated: true,
  appearance: "force-dark",

  head: [
    ["link", { rel: "icon", type: "image/svg+xml", href: "/favicon.svg" }],
    [
      "link",
      { rel: "icon", type: "image/png", sizes: "96x96", href: "/favicon-96x96.png" },
    ],
    ["link", { rel: "icon", href: "/favicon.ico" }],
    [
      "link",
      { rel: "apple-touch-icon", sizes: "180x180", href: "/apple-touch-icon.png" },
    ],
    ["link", { rel: "manifest", href: "/site.webmanifest" }],
    ["meta", { name: "theme-color", content: "#04090d" }],
    ["meta", { property: "og:type", content: "website" }],
    ["meta", { property: "og:url", content: "https://docs.tui-lipan.dev" }],
    ["meta", { name: "twitter:card", content: "summary" }],
    ["meta", { property: "og:title", content: LANDING_TITLE }],
    [
      "meta",
      {
        property: "og:description",
        content:
          "Component-based TUI framework for Rust - declarative components, layout, focus, overlays, and a rich widget set.",
      },
    ],
    ["link", { rel: "preconnect", href: "https://fonts.googleapis.com" }],
    [
      "link",
      { rel: "preconnect", href: "https://fonts.gstatic.com", crossorigin: "" },
    ],
    [
      "link",
      {
        rel: "stylesheet",
        href: "https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400;500;700;800&display=swap",
      },
    ],
  ],

  markdown: { theme: { light: "night-owl", dark: "night-owl" } },

  // The index page takes its title from the <h1>, which reads
  // "tui-lipan Documentation | tui-lipan". Name it for what the page is instead.
  // This lives here rather than in `index.md`'s frontmatter because GitHub
  // renders YAML frontmatter as a table at the top of the file, and that file
  // is also the repository's documentation index.
  transformPageData(pageData) {
    if (pageData.relativePath === "index.md") {
      pageData.title = LANDING_TITLE;
      pageData.titleTemplate = false;
    }
  },

  vite: { plugins: [repoLinksPlugin(srcDir)] },

  themeConfig: {
    // The app mark, same file the browser tab uses - it is the rounded variant,
    // so it needs no styling beyond a size.
    logo: "/favicon.svg",
    lipanVersion: LIPAN_VERSION,
    outline: [2, 3],
    nav: [
      { text: "Landing", link: "https://tui-lipan.dev" },
      { text: "Crates.io", link: "https://crates.io/crates/tui-lipan" },
      { text: "docs.rs", link: "https://docs.rs/tui-lipan" },
    ],
    sidebar: [
      { text: "Introduction", link: "/" },
      {
        text: "Getting Started",
        collapsed: false,
        items: [
          { text: "Quick Start", link: "/quick-start" },
          { text: "Tutorial", link: "/tutorial" },
          { text: "Examples", link: "/examples" },
        ],
      },
      {
        text: "Core Concepts",
        collapsed: false,
        items: [
          { text: "Components", link: "/components" },
          { text: "Testing", link: "/testing" },
          { text: "UI Macros", link: "/macros" },
          { text: "Events & Callbacks", link: "/events" },
          { text: "Focus System", link: "/focus" },
          { text: "Keybindings", link: "/keybindings" },
          { text: "Styling & Themes", link: "/styling" },
          { text: "Text Editing", link: "/text-editing" },
          { text: "Error Handling", link: "/error-handling" },
        ],
      },
      {
        text: "Widgets",
        collapsed: false,
        items: [
          { text: "Overview", link: "/widgets/" },
          { text: "Layout & Containers", link: "/widgets/layout" },
          { text: "Display", link: "/widgets/display" },
          { text: "Diagrams", link: "/widgets/diagrams" },
          { text: "Input", link: "/widgets/input" },
          { text: "Data", link: "/widgets/data" },
          { text: "Feedback & Status", link: "/widgets/feedback" },
          { text: "Overlays & Navigation", link: "/widgets/overlays" },
          { text: "Tabs", link: "/widgets/tabs" },
          { text: "Terminal", link: "/widgets/terminal" },
          { text: "Terminal Images", link: "/widgets/terminal-images" },
          { text: "Effects", link: "/widgets/effects" },
        ],
      },
      {
        text: "Advanced",
        collapsed: true,
        items: [
          { text: "Clipboard", link: "/clipboard" },
          { text: "Inline Mode", link: "/inline-mode" },
          { text: "External Programs", link: "/external-programs" },
          { text: "Patterns & Recipes", link: "/patterns" },
          { text: "Large App Shells", link: "/large-app-shells" },
          { text: "Performance", link: "/perf" },
          { text: "Web Backend", link: "/web-backend" },
        ],
      },
      {
        text: "Reference",
        collapsed: true,
        items: [
          { text: "Enum & Type Reference", link: "/enums" },
          { text: "Widget Defaults", link: "/widget-defaults" },
        ],
      },
      {
        text: "Contributing",
        collapsed: true,
        items: [
          { text: "Widget Authoring Guide", link: "/widget-authoring" },
          { text: "Architecture & Design", link: "/DESIGN" },
        ],
      },
    ],
    editLink: {
      pattern: "https://github.com/tui-lipan/tui-lipan/edit/main/docs/:path",
      text: "Edit this page on GitHub",
    },
    search: {
      provider: "local",
      options: {
        translations: {
          button: {
            buttonText: "Search...",
            buttonAriaLabel: "Search",
          },
        },
      },
    },
    footer: { message: "MPL-2.0", copyright: "© Adam Mikołajczyk" },
  },
});
