import type { Config } from "tailwindcss";

/**
 * Tailwind names for the design tokens in src/index.css (source of truth:
 * docs/design-system/tokens.css). Light theme only. The legacy shadcn names
 * (background, card, muted, primary, …) stay mapped onto the same tokens until
 * the last page is restyled, then they go.
 */
export default {
  content: ["./index.html", "./src/**/*.{js,jsx,ts,tsx}"],
  theme: {
    extend: {
      colors: {
        // ---- Design tokens ----
        canvas: { DEFAULT: "var(--canvas)", glow: "var(--canvas-glow)" },
        sidebar: { DEFAULT: "var(--sidebar)", 2: "var(--sidebar-2)" },
        paper: { DEFAULT: "var(--paper)", 2: "var(--paper-2)" },
        well: { DEFAULT: "var(--well)", 2: "var(--well-2)", 3: "var(--well-3)" },
        "hero-orb": "var(--hero-orb)",
        line: {
          DEFAULT: "var(--line)",
          soft: "var(--line-soft)",
          strong: "var(--line-strong)",
          sidebar: "var(--line-sidebar)",
          hover: "var(--line-hover)",
        },
        ink: {
          DEFAULT: "var(--ink)",
          2: "var(--ink-2)",
          3: "var(--ink-3)",
          4: "var(--ink-4)",
          5: "var(--ink-5)",
          inverse: "var(--ink-inverse)",
          "inverse-2": "var(--ink-inverse-2)",
          hover: "var(--ink-hover)",
        },
        dark: { DEFAULT: "var(--dark)", hi: "var(--dark-hi)", lo: "var(--dark-lo)" },
        gain: { DEFAULT: "var(--gain)", soft: "var(--gain-soft)" },
        loss: {
          DEFAULT: "var(--loss)",
          soft: "var(--loss-soft)",
          line: "var(--loss-line)",
          hover: "var(--loss-hover)",
        },
        s1: "var(--s1)",
        s2: "var(--s2)",
        s3: "var(--s3)",
        s4: "var(--s4)",
        "s-other": "var(--s-other)",
        chart: {
          line: "var(--chart-line)",
          area: "var(--chart-area)",
          grid: "var(--chart-grid)",
          cost: "var(--chart-cost)",
          axis: "var(--chart-axis)",
        },
        focus: "var(--focus-border)",
      },
      backgroundImage: {
        "dark-grad": "var(--dark-grad)",
        "dark-grad-hover": "var(--dark-grad-hover)",
        "paper-grad": "linear-gradient(145deg, var(--paper), var(--paper-2))",
        "stat-grad": "linear-gradient(150deg, var(--paper), var(--paper-2))",
        "btn-grad": "linear-gradient(var(--paper), var(--paper-2))",
        "sidebar-grad": "linear-gradient(100deg, var(--canvas-glow), var(--sidebar-2))",
      },
      borderRadius: {
        r1: "var(--r-1)",
        r2: "var(--r-2)",
        r3: "var(--r-3)",
        r4: "var(--r-4)",
        r5: "var(--r-5)",
      },
      boxShadow: {
        sidebar: "var(--sh-sidebar)",
        e1: "var(--sh-1)",
        e2: "var(--sh-2)",
        e3: "var(--sh-3)",
        pop: "var(--sh-pop)",
        modal: "var(--sh-modal)",
        dark: "var(--sh-dark)",
        btn: "var(--sh-btn)",
        "btn-hover": "var(--sh-btn-hover)",
        seg: "var(--sh-seg)",
        destructive: "var(--sh-destructive)",
        focus: "var(--focus-ring)",
        "loss-ring": "var(--loss-ring)",
      },
      fontFamily: {
        sans: ["var(--font)"],
      },
      // Type roles (README §3): size, line height, tracking and weight in one class.
      fontSize: {
        display: ["42px", { lineHeight: "1", letterSpacing: "-0.08em", fontWeight: "580" }],
        h1: ["36px", { lineHeight: "1.1", letterSpacing: "-0.07em", fontWeight: "570" }],
        modal: ["21px", { lineHeight: "1.2", letterSpacing: "-0.05em", fontWeight: "600" }],
        stat: ["22px", { lineHeight: "1.1", letterSpacing: "-0.06em", fontWeight: "580" }],
        h2: ["18px", { lineHeight: "1.2", letterSpacing: "-0.045em", fontWeight: "650" }],
        "h2-section": ["17px", { lineHeight: "1.2", letterSpacing: "-0.04em", fontWeight: "650" }],
        h3: ["16px", { lineHeight: "1.25", letterSpacing: "-0.04em", fontWeight: "620" }],
        nav: ["14px", { lineHeight: "1", letterSpacing: "-0.015em", fontWeight: "600" }],
        body: ["13px", { lineHeight: "1.5", fontWeight: "450" }],
        table: ["12px", { lineHeight: "1.3", fontWeight: "480" }],
        caption: ["11px", { lineHeight: "1.4", fontWeight: "500" }],
        micro: ["10px", { lineHeight: "1.4", fontWeight: "600" }],
        eyebrow: ["10px", { lineHeight: "1", letterSpacing: "0.12em", fontWeight: "700" }],
        thead: ["10px", { lineHeight: "1", letterSpacing: "0.08em", fontWeight: "750" }],
      },
      fontWeight: {
        450: "450",
        480: "480",
        550: "550",
        570: "570",
        580: "580",
        620: "620",
        650: "650",
        750: "750",
      },
      spacing: {
        sidebar: "var(--sidebar-w)",
        control: "var(--control-h)",
        "control-sm": "var(--control-h-sm)",
        input: "var(--input-h)",
        row: "var(--table-row-h)",
      },
      maxWidth: {
        page: "var(--page-max)",
      },
      transitionDuration: {
        fast: "120ms",
        base: "180ms",
      },
      transitionTimingFunction: {
        moony: "cubic-bezier(0.2, 0.7, 0.2, 1)",
      },
      keyframes: {
        "accordion-down": {
          from: { height: "0" },
          to: { height: "var(--radix-accordion-content-height)" },
        },
        "accordion-up": {
          from: { height: "var(--radix-accordion-content-height)" },
          to: { height: "0" },
        },
        "modal-in": {
          from: { opacity: "0", transform: "translate(-50%, calc(-50% + 10px)) scale(0.985)" },
          to: { opacity: "1", transform: "translate(-50%, -50%) scale(1)" },
        },
        "modal-out": {
          from: { opacity: "1", transform: "translate(-50%, -50%) scale(1)" },
          to: { opacity: "0", transform: "translate(-50%, calc(-50% + 10px)) scale(0.985)" },
        },
        "toast-in": {
          from: { opacity: "0", transform: "translateY(8px)" },
          to: { opacity: "1", transform: "none" },
        },
      },
      animation: {
        "accordion-down": "accordion-down 0.2s ease-out",
        "accordion-up": "accordion-up 0.2s ease-out",
        "modal-in": "modal-in 180ms cubic-bezier(0.2, 0.7, 0.2, 1)",
        "modal-out": "modal-out 180ms cubic-bezier(0.2, 0.7, 0.2, 1)",
        "toast-in": "toast-in 180ms cubic-bezier(0.2, 0.7, 0.2, 1)",
      },
    },
  },
  plugins: [require("tailwindcss-animate"), require("@tailwindcss/typography")],
} satisfies Config;
