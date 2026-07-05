import { defineConfig } from "vitepress";

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

  themeConfig: {
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
    footer: { message: "MIT OR Apache-2.0", copyright: "© Adam Mikołajczyk" },
  },
});
